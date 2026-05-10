import { invoke, listen, windowClose, windowMinimize, windowToggleMaximize } from './bridge.js';
import { State, $, setText, toast, escHtml, setSidebarCollapsed, nowTs } from './state.js';
import {
  appendChatMessage, renderAgentMessageParts, renderToolCall, renderToolResult,
  appendThinkingIndicator, removeThinkingIndicator, scrollToBottom,
} from './chat.js';
import { renderSessionsPage, setupSessionsHandlers } from './sessions.js';
import { loadSettings, saveSettings, setupSettingsHandlers, PROVIDER_MODELS, populateModelDropdown } from './settings.js';

async function init() {
  try {
    State.config = await invoke('agent_get_config');
  } catch {
    State.config = { agent_port: 4096, provider: 'gemini', model: '', api_key: '', gcp_project: '', cwd: '' };
  }

  setText('status-port', `port ${State.config.agent_port || 4096}`);
  setText('status-provider', State.config.provider || '--');
  setText('sb-provider', State.config.provider || '--');

  try {
    const health = await invoke('agent_health');
    if (health?.healthy) {
      State.agentHealthy = true;
      State.agentRunning = true;
      setAgentStatus('connected', `connected (v${health.version || '?'})`);
      setText('status-cwd', health.cwd || '');
    }
  } catch {
    setAgentStatus('disconnected', 'not running');
  }

  if (!State.agentHealthy) {
    try {
      await invoke('agent_start');
      await waitForHealth(8, 1000);
    } catch (e) {
      console.warn('[init] agent_start failed:', e);
    }
  }

  try {
    await listen('agent_event', handleAgentEvent);
  } catch (e) {
    console.warn('[init] listen failed:', e);
  }

  setupHandlers();
  setupSessionsHandlers();
  setupSettingsHandlers();
  setSidebarCollapsed(State.sidebarCollapsed);
  setText('sb-provider', State.config.provider || '--');
  navigate('home');
}

async function waitForHealth(maxRetries, delayMs) {
  for (let i = 1; i <= maxRetries; i++) {
    try {
      const health = await invoke('agent_health');
      if (health?.healthy) {
        State.agentHealthy = true;
        State.agentRunning = true;
        setAgentStatus('connected', `connected (v${health.version || '?'})`);
        setText('status-cwd', health.cwd || '');
        return true;
      }
    } catch {}
    if (i < maxRetries) await delay(delayMs);
  }
  setAgentStatus('disconnected', 'not running');
  return false;
}

function setAgentStatus(status, text) {
  const dot = $('status-dot');
  if (dot) {
    dot.className = 'statusbar-dot';
    if (status === 'connected') dot.classList.add('connected');
    else if (status === 'error') dot.classList.add('error');
  }
  setText('status-agent', text);
}

function delay(ms) { return new Promise(r => setTimeout(r, ms)); }

function handleAgentEvent(payload) {
  if (!payload) return;
  const { event, data } = payload;

  switch (event) {
    case 'session.created':
      console.log('[sse] session.created:', data);
      break;
    case 'session.deleted':
      console.log('[sse] session.deleted:', data);
      break;
    case 'message.created':
    case 'message.updated':
      if (data?.session_id === State.sessionId) {
        pollMessages(State.sessionId);
      }
      break;
    case 'permission.created':
      showPermissionDialog(data);
      break;
    case 'agent.completed':
      console.log('[sse] agent.completed');
      break;
    case 'agent.error':
      removeThinkingIndicator();
      appendChatMessage({ kind: 'error', content: String(data?.error || 'Agent error'), ts: nowTs() });
      setSessionActive(false);
      break;
  }
}

function showPermissionDialog(data) {
  if (!data) return;
  const overlay = document.createElement('div');
  overlay.className = 'permission-overlay';

  const card = document.createElement('div');
  card.className = 'permission-card';

  const title = document.createElement('div');
  title.className = 'permission-title';
  title.textContent = 'Permission Required';

  const desc = document.createElement('div');
  desc.className = 'permission-desc';
  desc.textContent = data.description || data.message || JSON.stringify(data, null, 2);

  const actions = document.createElement('div');
  actions.className = 'permission-actions';

  const denyBtn = document.createElement('button');
  denyBtn.className = 'btn-outline';
  denyBtn.textContent = 'Deny';
  denyBtn.onclick = async () => {
    overlay.remove();
    try {
      await invoke('agent_deny_permission', { sessionId: State.sessionId, permissionId: data.id || data.permission_id });
    } catch (e) { console.warn('deny failed:', e); }
  };

  const approveBtn = document.createElement('button');
  approveBtn.className = 'btn-send';
  approveBtn.textContent = 'Approve';
  approveBtn.onclick = async () => {
    overlay.remove();
    try {
      await invoke('agent_approve_permission', { sessionId: State.sessionId, permissionId: data.id || data.permission_id });
    } catch (e) { console.warn('approve failed:', e); }
  };

  actions.appendChild(denyBtn);
  actions.appendChild(approveBtn);
  card.appendChild(title);
  card.appendChild(desc);
  card.appendChild(actions);
  overlay.appendChild(card);
  document.body.appendChild(overlay);
}

// ── Navigation ────────────────────────────────────────────

export function navigate(page) {
  State.page = page;
  document.querySelectorAll('.sb-item').forEach(i => i.classList.toggle('active', i.dataset.page === page));
  document.querySelectorAll('.page').forEach(p => p.classList.toggle('active', p.id === `page-${page}`));

  const names = { home: 'Home', sessions: 'Sessions', settings: 'Settings' };
  setText('tl-page', names[page] || page);

  if (page === 'home') showHome();
  if (page === 'sessions') renderSessionsPage();
  if (page === 'settings') loadSettings();
}

function showHome() {
  if (State.sessionId && State.sessionActive) {
    showChatView();
  } else if (State.sessionId) {
    showChatView();
  } else {
    showHomeLanding();
  }
}

function showHomeLanding() {
  $('home-chat')?.classList.add('hidden');
  $('home-empty')?.classList.remove('hidden');
  setText('tl-page', 'Home');
}

function showChatView() {
  $('home-empty')?.classList.add('hidden');
  $('home-chat')?.classList.remove('hidden');
}

// ── Session management ────────────────────────────────────

function setSessionActive(active) {
  State.sessionActive = active;
  const stopBtn = $('btn-stop');
  const sendBtn = $('btn-chat-send');
  const homeBtn = $('btn-home-send');
  if (stopBtn) stopBtn.style.display = active ? '' : 'none';
  if (sendBtn) sendBtn.disabled = active;
  if (homeBtn) homeBtn.disabled = active;
}

async function startSession(prompt, mode) {
  if (!prompt.trim()) { toast('Enter a task description', 'error'); return; }
  if (!State.agentHealthy) {
    toast('mowis-agent is not running. Check Settings.', 'error');
    return;
  }

  const modePrefix = mode === 'plan_only' ? '[Plan Only] ' : mode === 'quick_fix' ? '[Quick Fix] ' : '';
  const fullPrompt = modePrefix + prompt.trim();

  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';

  setSessionActive(true);
  showChatView();
  setText('tl-page', 'Home');

  appendChatMessage({ kind: 'user', content: prompt.trim(), ts: nowTs() });
  appendThinkingIndicator();

  try {
    const title = prompt.trim().slice(0, 120);
    const session = await invoke('agent_create_session', { title });
    State.sessionId = session.id;
    State.lastMessageCount = 0;
    setText('compose-session-info', `session ${session.id.slice(0, 8)}`);
    setText('chat-session-title', title);

    await invoke('agent_send_message', { sessionId: session.id, text: fullPrompt });

    startPolling(session.id);
  } catch (err) {
    removeThinkingIndicator();
    appendChatMessage({ kind: 'error', content: String(err), ts: nowTs() });
    setSessionActive(false);
  }
}

function startPolling(sessionId) {
  stopPolling();
  State.lastMessageCount = 0;

  async function poll() {
    if (State.sessionId !== sessionId) return;
    await pollMessages(sessionId);
    if (State.sessionId === sessionId && State.sessionActive) {
      State.pollTimer = setTimeout(poll, 1000);
    }
  }

  State.pollTimer = setTimeout(poll, 500);
}

function stopPolling() {
  if (State.pollTimer) {
    clearTimeout(State.pollTimer);
    State.pollTimer = null;
  }
}

async function pollMessages(sessionId) {
  try {
    const messages = await invoke('agent_list_messages', { sessionId: sessionId });
    if (!messages || !Array.isArray(messages)) return;

    if (messages.length > State.lastMessageCount) {
      const newMessages = messages.slice(State.lastMessageCount);
      State.lastMessageCount = messages.length;

      removeThinkingIndicator();

      for (const msg of newMessages) {
        if (msg.role === 'user') continue;
        const parts = msg.parts || [];
        renderAgentMessageParts(parts);

        const hasFinish = parts.some(p => p.type === 'finish');
        if (hasFinish) {
          stopPolling();
          setSessionActive(false);
          return;
        }
      }

      const lastMsg = messages[messages.length - 1];
      if (lastMsg?.role === 'assistant') {
        const hasFinish = (lastMsg.parts || []).some(p => p.type === 'finish');
        if (!hasFinish && State.sessionActive) {
          appendThinkingIndicator();
        }
      }
    }
  } catch (e) {
    console.warn('[poll] error:', e);
  }
}

export async function loadSessionMessages(sessionId) {
  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';

  State.sessionId = sessionId;
  State.lastMessageCount = 0;
  setSessionActive(false);
  showChatView();

  try {
    const messages = await invoke('agent_list_messages', { sessionId: sessionId });
    if (!messages) return;

    State.lastMessageCount = messages.length;

    for (const msg of messages) {
      if (msg.role === 'user') {
        const textPart = (msg.parts || []).find(p => p.type === 'text');
        appendChatMessage({ kind: 'user', content: textPart?.text || '', ts: nowTs() });
      } else if (msg.role === 'assistant') {
        renderAgentMessageParts(msg.parts || []);
      }
    }

    setText('compose-session-info', `session ${sessionId.slice(0, 8)}`);
    setText('chat-session-title', sessionId.slice(0, 12));
    setText('tl-page', 'Home');
  } catch (e) {
    appendChatMessage({ kind: 'error', content: 'Failed to load session: ' + e, ts: nowTs() });
  }
}

async function sendChatMessage() {
  const input = $('chat-input');
  if (!input) return;
  const text = input.value.trim();
  if (!text) return;

  if (!State.sessionId || !State.agentHealthy) {
    toast('No active session or agent not available', 'error');
    return;
  }

  appendChatMessage({ kind: 'user', content: text, ts: nowTs() });
  input.value = '';
  autoResize.call(input);

  appendThinkingIndicator();
  setSessionActive(true);

  try {
    await invoke('agent_send_message', { sessionId: State.sessionId, text });
    startPolling(State.sessionId);
  } catch (err) {
    removeThinkingIndicator();
    appendChatMessage({ kind: 'error', content: `Failed to send: ${err}`, ts: nowTs() });
    setSessionActive(false);
  }
}

function autoResize() {
  this.style.height = 'auto';
  this.style.height = Math.min(this.scrollHeight, 120) + 'px';
}

// ── Window controls ───────────────────────────────────────

async function runWindowAction(action) {
  try {
    if (action === 'close') await windowClose();
    if (action === 'minimize') await windowMinimize();
    if (action === 'toggle_maximize') await windowToggleMaximize();
  } catch (e) {
    console.warn('[window]', action, 'failed:', e);
  }
}

function bindWindowControls(root = document) {
  const bindings = [
    ['.tl-red', 'close'],
    ['.tl-yellow', 'minimize'],
    ['.tl-green', 'toggle_maximize'],
  ];
  for (const [selector, action] of bindings) {
    root.querySelectorAll(selector).forEach(btn => {
      if (btn.dataset.windowControlBound) return;
      btn.dataset.windowControlBound = '1';
      btn.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        runWindowAction(action);
      });
    });
  }
}

// ── Handlers ──────────────────────────────────────────────

function setupHandlers() {
  bindWindowControls(document);

  document.querySelectorAll('.sb-item').forEach(item => {
    item.addEventListener('click', (e) => { e.preventDefault(); navigate(item.dataset.page); });
  });
  $('btn-sidebar-toggle')?.addEventListener('click', () => setSidebarCollapsed(!State.sidebarCollapsed));

  $('btn-home-send')?.addEventListener('click', () => {
    const p = $('home-input')?.value.trim();
    const m = $('home-mode')?.value;
    if (p) startSession(p, m);
  });
  $('home-input')?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      const p = $('home-input')?.value.trim();
      const m = $('home-mode')?.value;
      if (p) startSession(p, m);
    }
  });

  document.querySelectorAll('.suggestion').forEach(btn => {
    btn.addEventListener('click', () => {
      const ta = $('home-input');
      if (ta) { ta.value = btn.dataset.text; ta.focus(); }
    });
  });

  $('btn-chat-send')?.addEventListener('click', sendChatMessage);
  $('chat-input')?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChatMessage(); }
  });

  $('btn-stop')?.addEventListener('click', async () => {
    stopPolling();
    if (State.sessionId && State.agentHealthy) {
      try { await invoke('agent_abort', { sessionId: State.sessionId }); } catch {}
    }
    removeThinkingIndicator();
    setSessionActive(false);
    appendChatMessage({ kind: 'system', content: 'Session stopped.', ts: nowTs() });
    toast('Session stopped');
  });

  $('btn-chat-home')?.addEventListener('click', () => {
    State.sessionId = null;
    State.lastMessageCount = 0;
    const chatMessages = $('chat-messages');
    if (chatMessages) chatMessages.innerHTML = '';
    setText('compose-session-info', '');
    setText('chat-session-title', 'Session');
    setSessionActive(false);
    stopPolling();
    navigate('home');
  });

  $('btn-new-session')?.addEventListener('click', () => {
    State.sessionId = null;
    State.lastMessageCount = 0;
    const chatMessages = $('chat-messages');
    if (chatMessages) chatMessages.innerHTML = '';
    setText('compose-session-info', '');
    setText('chat-session-title', 'Session');
    setSessionActive(false);
    stopPolling();
    navigate('home');
  });

  $('btn-save-settings')?.addEventListener('click', saveSettings);

  $('chat-input')?.addEventListener('input', autoResize);

  $('btn-agent-start')?.addEventListener('click', async () => {
    toast('Starting mowis-agent...', 'info');
    try {
      await invoke('agent_start');
      const ok = await waitForHealth(10, 1000);
      if (ok) {
        toast('Agent started successfully', 'success');
      } else {
        toast('Agent failed to start', 'error');
      }
    } catch (e) {
      toast('Failed to start: ' + e, 'error');
    }
  });

  $('btn-agent-stop')?.addEventListener('click', async () => {
    try {
      await invoke('agent_stop');
      State.agentHealthy = false;
      State.agentRunning = false;
      setAgentStatus('disconnected', 'stopped');
      toast('Agent stopped');
    } catch (e) {
      toast('Failed to stop: ' + e, 'error');
    }
  });

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === '1') { e.preventDefault(); navigate('home'); }
    if ((e.metaKey || e.ctrlKey) && e.key === '2') { e.preventDefault(); navigate('sessions'); }
    if ((e.metaKey || e.ctrlKey) && e.key === ',') { e.preventDefault(); navigate('settings'); }
  });
}

init().catch(console.error);
