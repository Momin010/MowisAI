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
    State.config = { provider: 'anthropic', model: '', api_key: '', gcp_project: '', cwd: '' };
  }

  setText('status-provider', State.config.provider || '--');
  setText('sb-provider', State.config.provider || '--');

  try {
    const health = await invoke('agent_health');
    if (health?.healthy) {
      State.agentHealthy = true;
      State.agentRunning = true;
      setAgentStatus('connected', `ready (${health.version})`);
      setText('status-cwd', health.cwd || '');
    }
  } catch {
    setAgentStatus('disconnected', 'opencode not found');
  }

  if (!State.agentHealthy) {
    try {
      await invoke('agent_start');
      const health = await invoke('agent_health');
      if (health?.healthy) {
        State.agentHealthy = true;
        State.agentRunning = true;
        setAgentStatus('connected', `ready (${health.version})`);
      }
    } catch (e) {
      console.warn('[init] agent_start failed:', e);
    }
  }

  // Listen for agent events (progress, completion)
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
  const type = payload.type;

  switch (type) {
    case 'status':
      if (payload.session_id === State.sessionId) {
        console.log('[event] status:', payload.status);
      }
      break;

    case 'progress':
      if (payload.session_id === State.sessionId) {
        console.log('[event] progress:', payload.text);
      }
      break;

    case 'completed':
      if (payload.session_id === State.sessionId) {
        removeThinkingIndicator();
        if (payload.success && payload.response) {
          appendChatMessage({ kind: 'assistant', content: payload.response, ts: nowTs() });
        } else if (payload.response) {
          appendChatMessage({ kind: 'error', content: payload.response, ts: nowTs() });
        }
        setSessionActive(false);
        setText('compose-session-info', `session ${State.sessionId.slice(0, 8)}`);
      }
      break;

    default:
      console.log('[event] unknown:', payload);
  }
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
  if (State.sessionId) {
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
    toast('opencode not found. Check Settings.', 'error');
    return;
  }

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
    setText('compose-session-info', `session ${session.id.slice(0, 8)}`);
    setText('chat-session-title', title);

    // Fire and forget — events come back via agent_event listener
    await invoke('agent_send_message', { sessionId: session.id, text: prompt.trim() });
  } catch (err) {
    removeThinkingIndicator();
    appendChatMessage({ kind: 'error', content: String(err), ts: nowTs() });
    setSessionActive(false);
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
  } catch (err) {
    removeThinkingIndicator();
    appendChatMessage({ kind: 'error', content: `Failed to send: ${err}`, ts: nowTs() });
    setSessionActive(false);
  }
}

export async function loadSessionMessages(sessionId) {
  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';

  State.sessionId = sessionId;
  setSessionActive(false);
  showChatView();

  try {
    const messages = await invoke('agent_list_messages', { sessionId });
    if (!messages) return;

    for (const msg of messages) {
      appendChatMessage({
        kind: msg.role === 'user' ? 'user' : 'assistant',
        content: msg.content,
        ts: msg.timestamp * 1000,
      });
    }

    setText('compose-session-info', `session ${sessionId.slice(0, 8)}`);
    setText('tl-page', 'Home');
  } catch (e) {
    appendChatMessage({ kind: 'error', content: 'Failed to load session: ' + e, ts: nowTs() });
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
    const chatMessages = $('chat-messages');
    if (chatMessages) chatMessages.innerHTML = '';
    setText('compose-session-info', '');
    setText('chat-session-title', 'Session');
    setSessionActive(false);
    navigate('home');
  });

  $('btn-new-session')?.addEventListener('click', () => {
    State.sessionId = null;
    const chatMessages = $('chat-messages');
    if (chatMessages) chatMessages.innerHTML = '';
    setText('compose-session-info', '');
    setText('chat-session-title', 'Session');
    setSessionActive(false);
    navigate('home');
  });

  $('btn-save-settings')?.addEventListener('click', saveSettings);

  $('chat-input')?.addEventListener('input', autoResize);

  $('btn-agent-start')?.addEventListener('click', async () => {
    toast('Looking for opencode...', 'info');
    try {
      await invoke('agent_start');
      const health = await invoke('agent_health');
      if (health?.healthy) {
        State.agentHealthy = true;
        State.agentRunning = true;
        setAgentStatus('connected', `ready (${health.version})`);
        toast('opencode found', 'success');
      } else {
        toast('opencode binary not found', 'error');
      }
    } catch (e) {
      toast('Failed: ' + e, 'error');
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
