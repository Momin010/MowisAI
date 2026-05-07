/**
 * MowisAI Desktop — Main Entry Point
 * Tauri v2 + Vite. Graceful browser fallback.
 */

import { loadTauri, isTauri, invoke, listen, openDialogNative } from './bridge.js';
import { delay, nowTs, fmtNumber, fmtTs, escapeHtml } from './utils.js';
import {
  State, $, setText, toast, escHtml, setSidebarCollapsed,
  updateHomeAgentHint, renderRepoChip, syncCustomSelect,
} from './state.js';
import {
  appendChatMessage, appendAgentChunk, finalizeStreaming,
  appendFileChanges, scrollToBottom, renderDiffPanel,
  startAgentPolling, stopAgentPolling,
  appendThinkingIndicator, removeThinkingIndicator,
  setChatCallbacks,
} from './chat.js';
import { renderSessionsPage, setupSessionsHandlers } from './sessions.js';
import { renderUsagePage } from './usage.js';
import { loadSettings, saveSettings, isAgentMode, setupSettingsHandlers } from './settings.js';
import { initSpeechRecognition } from './speech.js';
import { initFileUpload, pendingAttachments, clearAttachments } from './file-upload.js';
import {
  showEngineSetupModal, setupEngineSetupModalHandlers,
  hideEngineSetupModal, setupRepoHandlers,
  showDeveloperBootstrap, setupDeveloperBootstrapHandlers,
  handlePointInstallationFlow,
} from './modals.js';

// ── Wire chat callbacks (breaks circular dependency) ─────────────────────────

setChatCallbacks({ setTaskPanelOpen, renderTaskPanel, setSessionActive });

// ── Navigation ────────────────────────────────────────────────────────────────

export function navigate(page, opts = {}) {
  State.page = page;
  document.querySelectorAll('.sb-item').forEach(i => i.classList.toggle('active', i.dataset.page === page));
  document.querySelectorAll('.page').forEach(p => p.classList.toggle('active', p.id === `page-${page}`));

  const names = { home: 'Home', sessions: 'Sessions', usage: 'Usage', settings: 'Settings' };
  setText('tl-page', names[page] || page);

  if (page === 'home' && !opts.preserveHomeMode) {
    showHomeLanding({ clearBackend: !State.sessionActive });
  }
  if (page === 'sessions') renderSessionsPage();
  if (page === 'usage') renderUsagePage();
  if (page === 'settings') loadSettings();
}

export async function showHomeLanding({ clearBackend = false } = {}) {
  State.homeMode = 'new';
  State.taskPanelOpen = false;
  State.selectedTaskId = null;
  $('home-chat')?.classList.add('hidden');
  $('home-chat')?.classList.remove('task-panel-open');
  $('home-empty')?.classList.remove('hidden');
  setText('tl-page', 'Home');
  setText('compose-session-info', '');
  setText('chat-session-title', 'Session');
  updateTaskPanelVisibility();

  if (clearBackend) {
    stopAgentPolling();
    State.sessionId = null;
    State.agentSessionId = null;
    State.tasks = {};
    State.streamingContent = '';
    State.isStreaming = false;
    const chatMessages = $('chat-messages');
    if (chatMessages) chatMessages.innerHTML = '';
    try { await invoke('clear_current_session'); } catch {}
    updateTaskPanelVisibility();
    updateStatusBar();
  }
}

export function showSessionShell() {
  State.homeMode = 'session';
  $('home-empty')?.classList.add('hidden');
  $('home-chat')?.classList.remove('hidden');
  updateTaskPanelVisibility();
}

export function renderSessionDetail(detail) {
  if (!detail) return;
  State.sessionId = detail.summary?.id || State.sessionId;
  State.sessionActive = detail.summary?.status === 'running';
  State.tasks = Object.fromEntries((detail.tasks || []).map(task => [task.id, task]));
  State.taskPanelOpen = Object.keys(State.tasks).length > 0;
  State.selectedTaskId = State.selectedTaskId && State.tasks[State.selectedTaskId]
    ? State.selectedTaskId
    : Object.keys(State.tasks)[0] || null;
  State.stats.tokens_total = detail.tokens_total || 0;
  State.stats.tool_calls = detail.tool_calls_total || 0;

  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';
  State.isStreaming = false;
  State.streamingContent = '';
  (detail.messages || []).forEach(msg => {
    appendChatMessage(msg.kind === 'agent' ? { ...msg, streaming: false } : msg);
  });

  setText('compose-session-info', State.sessionId ? `session ${State.sessionId.slice(0,12)}` : '');
  setText('chat-session-title', detail.summary?.prompt || 'Session');
  setSessionActive(State.sessionActive, true);
  renderTaskPanel();
  updateStatusBar();
  showSessionShell();
}

// ── Window controls (decorations: false) ─────────────────────────────────────

async function runWindowAction(action) {
  if (!isTauri()) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (action === 'close') await win.close();
    if (action === 'minimize') await win.minimize();
    if (action === 'toggle_maximize') await win.toggleMaximize();
  } catch {}
}

export function bindWindowControls(root = document) {
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

async function setupWindowControls() {
  bindWindowControls(document);
}

// ── Splash ────────────────────────────────────────────────────────────────────

async function runSplash() {
  const fill = $('splash-fill');
  const hint = $('splash-hint');
  const terminalBody = $('boot-terminal-body');
  const continueBtn = $('splash-continue');

  if (fill) fill.style.width = '5%';
  if (hint) hint.textContent = 'Starting…';

  const kindIcons = {
    info: '\u2022',
    command: '\u25B6',
    output: ' ',
    success: '\u2713',
    error: '\u2717',
    warning: '\u26A0',
  };

  function appendBootLine(p) {
    if (!terminalBody) return;
    const line = document.createElement('div');
    line.className = `boot-line kind-${p.kind || 'info'}`;

    const ts = document.createElement('span');
    ts.className = 'boot-ts';
    const d = p.timestamp ? new Date(p.timestamp) : new Date();
    ts.textContent = d.toLocaleTimeString('en-GB', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });

    const icon = document.createElement('span');
    icon.className = 'boot-icon';
    icon.textContent = kindIcons[p.kind] || '\u2022';

    const msg = document.createElement('span');
    msg.className = 'boot-msg';
    msg.textContent = p.message;

    line.appendChild(ts);
    line.appendChild(icon);
    line.appendChild(msg);
    terminalBody.appendChild(line);

    if (p.detail) {
      const detailLine = document.createElement('div');
      detailLine.className = 'boot-line-detail';
      detailLine.textContent = p.detail;
      terminalBody.appendChild(detailLine);
    }

    terminalBody.scrollTop = terminalBody.scrollHeight;
  }

  let unlisten = null;
  let resolved = false;
  let lastErrorMessage = null;
  let lastErrorDetail = null;
  const backendReady = new Promise(resolve => {
    listen('setup_progress', (e) => {
      const p = e.payload;
      if (!p) return;
      if (fill) fill.style.width = Math.max(5, p.pct) + '%';
      if (hint) hint.textContent = p.message;
      appendBootLine(p);
      if (p.stage === 'error') {
        lastErrorMessage = p.message;
        lastErrorDetail = p.detail || null;
      }
      if ((p.stage === 'ready' || p.stage === 'error') && !resolved) {
        resolved = true;
        resolve(p);
      }
    }).then(u => { unlisten = u; });
  });

  const showContinueTimer = delay(3000).then(() => {
    if (continueBtn) continueBtn.classList.remove('hidden');
  });

  let earlyDismiss = null;
  if (continueBtn) {
    continueBtn.addEventListener('click', () => {
      if (earlyDismiss) earlyDismiss();
    }, { once: true });
  }

  const result = await Promise.race([
    backendReady,
    delay(90000).then(() => ({ stage: 'timeout' })),
    new Promise(resolve => { earlyDismiss = resolve; }),
  ]);
  if (unlisten) unlisten();

  if (lastErrorMessage) State.setupError = lastErrorMessage;

  if (fill) fill.style.width = '100%';
  await delay(200);
  $('splash')?.classList.add('hidden');
  $('app')?.classList.remove('hidden');

  if (result && (result.stage === 'error' || result.stage === 'timeout')) {
    const fullError = lastErrorDetail
      ? `${lastErrorMessage || 'Connection failed'}\n\n${lastErrorDetail}`
      : (lastErrorMessage || 'Connection timed out');
    showEngineSetupModal(fullError);
  }
}

// ── Init ──────────────────────────────────────────────────────────────────────

async function init() {
  await loadTauri();
  setupWindowControls();
  setupEngineSetupModalHandlers();
  await runSplash();

  try { State.config = await invoke('get_config'); } catch { State.config = { socket_path: '/tmp/agentd.sock', max_agents: 100, mode: 'auto', provider: 'gemini', model: 'gemini-2.0-flash', api_key: '', gcp_project: '', gcp_region: 'us-central1', gcp_service_account_key_path: '' }; }

  try {
    const info = await invoke('get_system_info');
    setText('about-meta', `${info.os} · ${info.arch} · MowisAI v${info.version}`);
    setText('tl-version', `v${info.version}`);
  } catch {}

  await maybeShowWelcome();

  await checkDaemonWithGuidance();

  // Check mowis-agent health (retry up to 10 times with 1s delay — agent may still be starting)
  const splashHint = $('splash-hint');
  for (let attempt = 1; attempt <= 10; attempt++) {
    if (splashHint) splashHint.textContent = `Connecting to agent... (${attempt}/10)`;
    console.log(`[agent] Health check attempt ${attempt}/10`);
    try {
      const health = await invoke('agent_health');
      if (health?.healthy) {
        State.agentHealthy = true;
        console.log(`[agent] ✓ Connected — v${health.version}, cwd: ${health.cwd}`);
        if (splashHint) splashHint.textContent = `Agent connected (v${health.version})`;
        break;
      } else {
        console.log(`[agent] Responded but not healthy:`, health);
      }
    } catch (e) {
      console.warn(`[agent] Attempt ${attempt}/10 failed:`, e);
      if (attempt === 10) {
        console.log('[agent] ✗ Not available after 10 attempts — falling back to simulation mode');
        if (splashHint) splashHint.textContent = 'Agent not available — using simulation mode';
      }
    }
    if (attempt < 10) await delay(1000);
  }

  try { await setupListeners(); } catch (e) { console.error('Listener setup failed:', e); }

  setupHandlers();
  initCustomSelects();
  initSpeechRecognition();
  initFileUpload();
  setSidebarCollapsed(State.sidebarCollapsed);
  await restoreInitialSession();

  setText('sb-provider', State.config?.provider || '—');

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === '1') { e.preventDefault(); navigate('home'); }
    if ((e.metaKey || e.ctrlKey) && e.key === '2') { e.preventDefault(); navigate('sessions'); }
    if ((e.metaKey || e.ctrlKey) && e.key === '3') { e.preventDefault(); navigate('usage'); }
    if ((e.metaKey || e.ctrlKey) && e.key === ',') { e.preventDefault(); navigate('settings'); }
  });
}

// ── Welcome screen (first launch) ─────────────────────────────────────────────

async function maybeShowWelcome() {
  if (localStorage.getItem('mowis_welcomed')) return;

  const welcome   = $('welcome');
  const word      = $('welcome-word');
  const cont      = $('welcome-continue');
  const btn       = $('btn-welcome-continue');
  if (!welcome) return;

  welcome.classList.remove('hidden');

  bindWindowControls(welcome);

  await delay(200);
  word?.classList.add('clear');

  await delay(2200);
  cont?.classList.add('visible');

  await new Promise(resolve => {
    btn?.addEventListener('click', resolve, { once: true });
    document.addEventListener('keydown', function handler(e) {
      if (e.key === 'Enter' || e.key === ' ') {
        document.removeEventListener('keydown', handler);
        resolve();
      }
    });
  });

  welcome.style.opacity  = '0';
  welcome.style.transition = 'opacity 0.55s ease';
  await delay(560);
  welcome.classList.add('hidden');
  welcome.style.opacity  = '';
  welcome.style.transition = '';

  localStorage.setItem('mowis_welcomed', '1');
}

// ── Daemon check ──────────────────────────────────────────────────────────────

async function checkDaemon() {
  try {
    const on = await invoke('check_daemon');
    setDaemonStatus(on);
    if (on) removeOfflineBanner();
  } catch { setDaemonStatus(false); }
  setTimeout(checkDaemon, 8000);
}

async function checkDaemonWithGuidance() {
  let connected = false;
  let os = 'unknown';
  let launcher = '';
  try {
    const cs = await invoke('get_connection_state');
    connected = cs.connected;
    launcher = cs.launcher || '';
  } catch {
    try { connected = await invoke('check_daemon'); } catch {}
  }
  try {
    const info = await invoke('get_system_info');
    os = info.os;
  } catch {}

  setDaemonStatus(connected);

  if (!connected) {
    showOfflineBanner(os, launcher);
  }

  setTimeout(checkDaemon, 8000);
}

const OFFLINE_GUIDANCE = {
  windows: `MowisAI needs a Linux engine to run. It will automatically install one via WSL2.\n\nIf setup is taking a while: open PowerShell as Administrator and run <code>wsl --install</code>, then restart the app.`,
  macos: `MowisAI needs QEMU to run the Linux engine.\n\nInstall it with: <code>brew install qemu</code>\n\nThen restart the app.`,
  linux: `The agentd daemon is not running.\n\nStart it with: <code>sudo agentd socket --path /tmp/agentd.sock</code>`,
  unknown: `The agent engine is not connected. Check the Settings tab to verify your socket path.`,
};

function showOfflineBanner(os, launcher) {
  if ($('offline-banner')) return;
  const guidance = OFFLINE_GUIDANCE[os] || OFFLINE_GUIDANCE.unknown;
  const errorBlock = State.setupError
    ? `<div class="offline-banner-error"><strong>Setup error:</strong><br><code>${escapeHtml(State.setupError)}</code></div>`
    : '';
  const logsBtn = `<button class="offline-banner-logs-btn" id="btn-show-engine-logs">Show engine logs</button>`;
  const banner = document.createElement('div');
  banner.id = 'offline-banner';
  banner.className = 'offline-banner';
  banner.innerHTML = `
    <div class="offline-banner-title">⚙ Engine not connected</div>
    <div class="offline-banner-body">${guidance.replace(/\n/g, '<br>')}</div>
    ${errorBlock}
    <div class="offline-banner-footer">
      The app is fully usable — sessions will run in simulation mode until the engine is available.
      ${logsBtn}
    </div>
    <pre class="offline-banner-logs hidden" id="offline-banner-logs"></pre>`;
  const homeEmpty = $('home-empty');
  if (homeEmpty) homeEmpty.prepend(banner);

  $('btn-show-engine-logs')?.addEventListener('click', async () => {
    const pre = $('offline-banner-logs');
    if (!pre) return;
    if (!pre.classList.contains('hidden')) {
      pre.classList.add('hidden');
      return;
    }
    pre.textContent = 'Loading logs…';
    pre.classList.remove('hidden');
    try {
      const logs = await invoke('get_engine_logs');
      pre.textContent = logs || '(no logs available)';
    } catch (e) {
      pre.textContent = `Error fetching logs: ${e}`;
    }
  });
}

function removeOfflineBanner() {
  $('offline-banner')?.remove();
}

function setDaemonStatus(on) {
  State.daemonConnected = on;
  const sbDot = $('sb-daemon-dot');
  const sbLabel = $('sb-daemon-label');

  if (sbDot)   sbDot.classList.toggle('on', on);
  if (sbLabel) sbLabel.textContent = on ? 'daemon online' : 'daemon offline';
}

// ── Tauri event listeners ─────────────────────────────────────────────────────

async function setupListeners() {
  await listen('daemon_status', (e) => setDaemonStatus(e.payload?.connected));

  await listen('chat_message', (e) => {
    const msg = e.payload;
    if (!msg) return;
    appendChatMessage(msg);
  });

  await listen('agent_chunk', (e) => {
    const chunk = e.payload?.chunk;
    if (!chunk) return;
    appendAgentChunk(chunk);
  });

  await listen('file_changes', (e) => {
    const changes = e.payload;
    if (!changes || !Array.isArray(changes) || changes.length === 0) return;
    appendFileChanges(changes);
    State.fileChanges.unshift({ ts: nowTs(), changes });
    State.fileChanges = State.fileChanges.slice(0, 20);

    State.taskPanelOpen = true;
    $('home-chat')?.classList.add('task-panel-open');
    renderDiffPanel();
  });

  await listen('task_added', (e) => {
    const task = e.payload;
    if (!task) return;
    const hadTasks = Object.keys(State.tasks).length > 0;
    State.tasks[task.id] = task;
    if (!hadTasks && State.homeMode === 'session') State.taskPanelOpen = true;
    renderTaskPanel();
    updateTaskPanelVisibility();
  });

  await listen('task_updated', (e) => {
    const { id, status } = e.payload || {};
    if (id && State.tasks[id]) {
      State.tasks[id].status = status;
      if (status === 'running') State.tasks[id].started_at = nowTs();
      if (status === 'complete' || status === 'failed') State.tasks[id].completed_at = nowTs();
    }
    renderTaskPanel();
    updateStatusBar();
  });

  await listen('stats_tick', (e) => {
    const s = e.payload || {};
    if (s.tokens_total !== undefined) State.stats.tokens_total = s.tokens_total;
    updateStatusBar();
  });

  await listen('session_complete', async () => {
    finalizeStreaming();

    try {
      const hist = await invoke('get_session_history');
      if (State.page === 'sessions') renderSessionsPage();
      setText('sb-badge-sessions', String(hist.length));
    } catch {}

    updateStatusBar();
  });
}

// ── Agent startup modal ───────────────────────────────────────────────────────

async function startAgentWithModal() {
  return new Promise((resolve) => {
    const modal    = $('agent-startup-modal');
    const logEl    = $('agent-startup-log');
    const spinner  = $('agent-startup-spinner');
    const spinnerText = $('agent-startup-spinner-text');
    const subtitle = $('agent-startup-subtitle');
    const errorEl  = $('agent-startup-error');
    const errorMsg = $('agent-startup-error-msg');
    const actionsEl = $('agent-startup-actions');
    const cancelBtn = $('btn-agent-startup-cancel');
    const retryBtn  = $('btn-agent-startup-retry');

    if (!modal) { resolve(false); return; }

    let unlistenLog = null;
    const cleanups = [];

    function addLogLine(text, level) {
      if (!logEl) return;
      const line = document.createElement('div');
      line.className = `agent-log-line ${level || 'info'}`;
      const pre = document.createElement('span');
      pre.className = 'agent-log-prefix';
      pre.textContent = level === 'success' ? '✓' : level === 'error' ? '✗' : '›';
      const txt = document.createElement('span');
      txt.textContent = text;
      line.appendChild(pre);
      line.appendChild(txt);
      logEl.appendChild(line);
      logEl.scrollTop = logEl.scrollHeight;
    }

    function resetUI() {
      if (logEl) logEl.innerHTML = '';
      if (errorEl) errorEl.classList.add('hidden');
      if (actionsEl) actionsEl.classList.add('hidden');
      if (spinner) { spinner.classList.remove('done', 'error'); }
      if (subtitle) subtitle.textContent = 'Connecting to the agent process...';
      if (spinnerText) spinnerText.textContent = 'Starting up...';
    }

    function closeModal() {
      modal.classList.add('hidden');
      modal.setAttribute('aria-hidden', 'true');
      cleanups.forEach(fn => fn());
    }

    // Subscribe to live log events from the backend
    listen('agent_startup_log', (e) => {
      addLogLine(e.payload?.text, e.payload?.level);
    }).then(u => { unlistenLog = u; cleanups.push(u); });

    function tryStart() {
      invoke('agent_start')
        .then(() => {
          if (spinner) spinner.classList.add('done');
          if (spinnerText) spinnerText.textContent = 'Agent connected';
          if (subtitle) subtitle.textContent = 'mowis-agent is ready';
          State.agentHealthy = true;
          setTimeout(() => { closeModal(); resolve(true); }, 500);
        })
        .catch((err) => {
          if (spinner) spinner.classList.add('error');
          if (spinnerText) spinnerText.textContent = 'Failed to start';
          if (subtitle) subtitle.textContent = 'Agent could not be started';
          if (errorEl) errorEl.classList.remove('hidden');
          if (errorMsg) errorMsg.textContent = String(err);
          if (actionsEl) actionsEl.classList.remove('hidden');
        });
    }

    const onCancel = () => { closeModal(); resolve(false); };
    const onRetry  = () => { resetUI(); tryStart(); };
    cancelBtn?.addEventListener('click', onCancel);
    retryBtn?.addEventListener('click', onRetry);
    cleanups.push(() => cancelBtn?.removeEventListener('click', onCancel));
    cleanups.push(() => retryBtn?.removeEventListener('click', onRetry));

    resetUI();
    modal.classList.remove('hidden');
    modal.setAttribute('aria-hidden', 'false');
    tryStart();
  });
}

// ── Session start ─────────────────────────────────────────────────────────────

export async function startSession(prompt, mode, repo = State.selectedRepo) {
  if (!prompt.trim() && pendingAttachments.length === 0) { toast('Enter a task description', 'error'); return; }

  const fullPrompt = prompt.trim();
  const agentModeRequested = mode === 'agent';
  clearAttachments();

  State.tasks = {};
  State.selectedTaskId = null;
  State.taskPanelOpen = false;
  State.isStreaming = false;
  State.streamingContent = '';
  State.planCardShown = false;
  State.lastMessageCount = 0;

  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';

  const taskPanelBody = $('task-panel-body');
  if (taskPanelBody) taskPanelBody.innerHTML = '';

  showSessionShell();
  setSessionActive(true);

  appendChatMessage({ kind: 'user', content: fullPrompt, ts: nowTs() });
  if (repo?.path) {
    appendChatMessage({
      kind: 'system',
      content: `Repository attached: ${repo.name || 'repository'} (${repo.path})`,
      ts: nowTs(),
    });
  }

  updateTaskPanelVisibility();

  try {
    // Re-check health if we think it's unhealthy
    if (!State.agentHealthy) {
      try {
        const health = await invoke('agent_health');
        State.agentHealthy = health?.healthy === true;
        if (State.agentHealthy) console.log('[session] Agent reconnected:', health.version);
      } catch (e) {
        console.warn('[session] Agent re-check failed:', e);
        State.agentHealthy = false;
      }
    }

    // When agent mode is explicitly requested and agent isn't running,
    // show the startup modal — never silently fall back to simulation.
    if (agentModeRequested && !State.agentHealthy) {
      console.log('[session] Agent mode requested but not healthy — showing startup modal');
      const started = await startAgentWithModal();
      if (!started) {
        // User cancelled or agent failed — abort cleanly, no simulation
        removeThinkingIndicator();
        setSessionActive(false);
        navigate('home', { preserveHomeMode: true });
        return;
      }
    }

    if (State.agentHealthy) {
      const title = fullPrompt.slice(0, 120);
      console.log('[session] Creating agent session:', title);
      const session = await invoke('agent_create_session', { title });
      State.agentSessionId = session.id;
      State.sessionId = session.id;
      console.log('[session] ✓ Session created:', session.id);
      setText('compose-session-info', `session ${session.id.slice(0, 8)}`);
      setText('chat-session-title', fullPrompt.slice(0, 120));

      appendThinkingIndicator();
      console.log('[session] Sending message (async)...');
      await invoke('agent_send_message', {
        sessionId: session.id,
        text: fullPrompt,
        background: true,
      });
      console.log('[session] ✓ Message sent, starting polling');
      startAgentPolling(session.id);
    } else if (agentModeRequested) {
      // Startup modal succeeded but health is still false — shouldn't happen, but guard it
      throw new Error('mowis-agent is not available. Check that the binary is installed alongside the app.');
    } else {
      // Non-agent mode: use orchestration (real daemon or simulation fallback)
      console.log('[session] Using orchestration mode:', mode || 'auto');
      const id = await invoke('start_session', {
        prompt: fullPrompt,
        mode: mode || 'auto',
        projectPath: repo?.path || null,
        repoUrl: repo?.repo_url || repo?.remote_url || null,
        repoSource: repo?.source || null,
        images: null,
      });
      State.sessionId = id;
      setText('compose-session-info', `session ${id.slice(0, 12)}`);
      setText('chat-session-title', fullPrompt.slice(0, 120));
    }
  } catch (err) {
    removeThinkingIndicator();
    appendChatMessage({ kind: 'error', content: String(err), ts: nowTs() });
    setSessionActive(false);
  }

  navigate('home', { preserveHomeMode: true });
}

export function setSessionActive(active, keepChat = false) {
  State.sessionActive = active;
  const stopBtn = $('btn-stop');
  const sendBtn = $('btn-chat-send');
  const homeBtn = $('btn-home-send');
  if (stopBtn) stopBtn.style.display = active ? '' : 'none';
  if (sendBtn) sendBtn.disabled = active;
  if (homeBtn) homeBtn.disabled = active;
}

// ── Task panel ────────────────────────────────────────────────────────────────

function updateTaskPanelVisibility() {
  const panel = $('task-panel');
  const chat = $('home-chat');
  const openBtn = $('task-panel-open');
  const hasTasks = Object.keys(State.tasks).length > 0;
  const hasChanges = State.fileChanges && State.fileChanges.length > 0;
  if (openBtn) openBtn.style.display = (hasTasks || hasChanges) ? '' : 'none';
  if (!hasTasks && !hasChanges) State.taskPanelOpen = false;
  if (panel) panel.style.display = (hasTasks || hasChanges) && State.taskPanelOpen ? '' : 'none';
  if (chat) chat.classList.toggle('task-panel-open', (hasTasks || hasChanges) && State.taskPanelOpen);
}

function renderTaskPanel() {
  if (State.fileChanges && State.fileChanges.length > 0) {
    renderDiffPanel();
    updateTaskPanelVisibility();
    return;
  }
  const body = $('task-panel-body');
  const counts = $('task-counts');
  const subtitle = $('task-panel-subtitle');
  const fill = $('task-progress-fill');
  if (!body) return;

  const tasks = Object.values(State.tasks);
  const done = tasks.filter(t => t.status === 'complete').length;
  const total = tasks.length;

  if (counts) counts.textContent = `${done} / ${total}`;
  if (subtitle) subtitle.textContent = `${done} completed`;
  if (fill) fill.style.width = total > 0 ? `${(done / total * 100).toFixed(1)}%` : '0%';

  if (!State.selectedTaskId || !State.tasks[State.selectedTaskId]) {
    State.selectedTaskId = tasks[0]?.id || null;
  }

  body.innerHTML = tasks.map(t => `
    <div class="task-row ${State.selectedTaskId === t.id ? 'selected' : ''}" data-id="${escHtml(t.id)}">
      <span class="task-dot ${t.status}"></span>
      <span class="task-desc">${escHtml(t.description)}</span>
      <span class="task-sb">${t.sandbox || ''}</span>
    </div>`).join('');

  body.querySelectorAll('.task-row').forEach(row => {
    row.addEventListener('click', () => {
      State.selectedTaskId = row.dataset.id;
      State.taskPanelOpen = true;
      renderTaskPanel();
      updateTaskPanelVisibility();
    });
  });

  renderTaskDetail();
  updateTaskPanelVisibility();
  updateStatusBar();
}

function renderTaskDetail() {
  if (State.fileChanges && State.fileChanges.length > 0) {
    renderDiffPanel();
    return;
  }
  const el = $('task-detail');
  if (!el) return;
  const task = State.selectedTaskId ? State.tasks[State.selectedTaskId] : null;
  if (!task) {
    el.innerHTML = '<div class="task-detail-empty">Select a task to inspect its implementation details.</div>';
    return;
  }
  const files = task.files || [];
  const views = task.views || [];
  el.innerHTML = `
    <div class="task-detail-title">${escHtml(task.description)}</div>
    <div class="task-detail-meta">
      <span class="task-pill">${escHtml(task.status || 'pending')}</span>
      ${task.sandbox ? `<span class="task-pill">${escHtml(task.sandbox)}</span>` : ''}
      ${task.completed_at ? `<span class="task-pill">${fmtTs(task.completed_at)}</span>` : ''}
    </div>
    <div class="task-detail-empty">${escHtml(task.summary || 'No implementation summary reported yet.')}</div>
    <div class="task-detail-section">
      <h4>Files</h4>
      <div class="task-detail-list">
        ${files.length ? files.map(f => `<div>${escHtml(f)}</div>`).join('') : '<div>No file changes reported yet</div>'}
      </div>
    </div>
    <div class="task-detail-section">
      <h4>Views</h4>
      <div class="task-detail-list">
        ${views.length ? views.map(v => `<div>${escHtml(v)}</div>`).join('') : '<div>No view metadata reported yet</div>'}
      </div>
    </div>
  `;
}

function setTaskPanelOpen(open) {
  const hasTasks = Object.keys(State.tasks).length > 0;
  const hasChanges = State.fileChanges && State.fileChanges.length > 0;
  State.taskPanelOpen = open && (hasTasks || hasChanges);
  updateTaskPanelVisibility();
  if (State.taskPanelOpen) renderTaskPanel();
}

// ── Status bar ───────────────────────────────────────────────────────────────

function updateStatusBar() {
  // Status bar removed — no-op
}

// ── Custom selects ────────────────────────────────────────────────────────────

function initCustomSelects() {
  document.querySelectorAll('select.mini-select, select.form-select').forEach(select => {
    if (select.dataset.customSelectReady) return;
    select.dataset.customSelectReady = '1';
    select.classList.add('native-select-hidden');

    const wrap = document.createElement('div');
    wrap.className = 'custom-select';
    wrap.dataset.selectId = select.id;

    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'custom-select-button';

    const menu = document.createElement('div');
    menu.className = 'custom-select-menu';

    wrap.appendChild(button);
    wrap.appendChild(menu);
    select.insertAdjacentElement('afterend', wrap);

    button.addEventListener('click', (e) => {
      e.preventDefault();
      document.querySelectorAll('.custom-select.open').forEach(other => {
        if (other !== wrap) other.classList.remove('open');
      });
      wrap.classList.toggle('open');
    });

    syncCustomSelect(select);
  });

  document.addEventListener('click', (e) => {
    if (!e.target.closest('.custom-select')) {
      document.querySelectorAll('.custom-select.open').forEach(wrap => wrap.classList.remove('open'));
    }
  });
}

// ── Handlers ──────────────────────────────────────────────────────────────────

function setupHandlers() {
  const taskClose = $('task-panel-toggle');
  if (taskClose) taskClose.textContent = 'x';

  // Nav
  document.querySelectorAll('.sb-item').forEach(item => {
    item.addEventListener('click', (e) => { e.preventDefault(); navigate(item.dataset.page); });
  });
  $('btn-sidebar-toggle')?.addEventListener('click', () => setSidebarCollapsed(!State.sidebarCollapsed));
  $('btn-chat-home')?.addEventListener('click', () => navigate('home'));

  // Repo (delegated to modals module)
  setupRepoHandlers();

  // Home send
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

  $('home-mode')?.addEventListener('change', (e) => {
    updateHomeAgentHint(e.target.value);
    syncCustomSelect($('home-mode'));
  });

  // Suggestions
  document.querySelectorAll('.suggestion').forEach(btn => {
    btn.addEventListener('click', () => {
      const ta = $('home-input');
      if (ta) { ta.value = btn.dataset.text; ta.focus(); }
    });
  });

  // Chat send
  $('btn-chat-send')?.addEventListener('click', sendChatMessage);
  $('chat-input')?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChatMessage(); }
  });

  // Stop
  $('btn-stop')?.addEventListener('click', async () => {
    stopAgentPolling();
    if (State.agentSessionId && State.agentHealthy) {
      try { await invoke('agent_abort', { sessionId: State.agentSessionId }); } catch {}
    }
    await invoke('stop_session');
    finalizeStreaming();
    removeThinkingIndicator();
    setSessionActive(false, true);
    State.agentSessionId = null;
    const sys = { kind: 'system', content: 'Session stopped.', ts: nowTs() };
    appendChatMessage(sys);
    toast('Session stopped');
  });

  // Task panel toggle
  $('task-panel-open')?.addEventListener('click', () => setTaskPanelOpen(true));
  $('task-panel-toggle')?.addEventListener('click', () => {
    setTaskPanelOpen(false);
    const btn = null;
    return;
    if (btn) btn.textContent = State.taskPanelOpen ? '▲' : '▼';
  });

  // Sessions → new session
  $('btn-new-session')?.addEventListener('click', () => navigate('home'));
  setupSessionsHandlers();

  // Settings
  $('btn-save-settings')?.addEventListener('click', saveSettings);
  setupSettingsHandlers();

  $('btn-engine-point')?.addEventListener('click', async () => {
    toast('Point to your QEMU installation', 'info');
    await handlePointInstallationFlow();
  });
  $('btn-engine-developer')?.addEventListener('click', () => {
    showDeveloperBootstrap();
  });

  $('btn-discard-sandbox')?.addEventListener('click', async () => {
    try {
      await invoke('discard_sandbox');
      toast('Sandbox discarded', 'success');
    } catch (err) {
      toast('Could not discard sandbox: ' + err, 'error');
    }
  });
  $('btn-test-socket')?.addEventListener('click', async () => {
    const statusEl = $('socket-status');
    try {
      const on = await invoke('check_daemon');
      if (statusEl) { statusEl.textContent = on ? '✓ Connected' : '✗ Not reachable'; statusEl.className = 'socket-status ' + (on ? 'ok' : 'err'); }
      toast(on ? 'Daemon connected' : 'Daemon not reachable', on ? 'success' : 'error');
    } catch { if (statusEl) { statusEl.textContent = '✗ Error'; statusEl.className = 'socket-status err'; } }
  });

  $('btn-browse-sa-key')?.addEventListener('click', async () => {
    try {
      const selected = await openDialogNative({
        title: 'Select GCP Service Account Key File',
        multiple: false,
        directory: false,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (selected) {
        const path = Array.isArray(selected) ? selected[0] : selected;
        const el = $('set-sa-key');
        if (el) el.value = path;
      }
    } catch (e) {
      console.error('File dialog error:', e);
    }
  });

  // Auto-resize compose textarea
  $('chat-input')?.addEventListener('input', autoResize);
}

async function sendChatMessage() {
  const input = $('chat-input');
  if (!input) return;
  const text = input.value.trim();
  if (!text && pendingAttachments.length === 0) return;

  const fullText = text;
  clearAttachments();

  appendChatMessage({ kind: 'user', content: fullText, ts: nowTs() });
  input.value = '';
  autoResize.call(input);

  if (State.agentSessionId && State.agentHealthy) {
    console.log('[chat] Sending follow-up to session:', State.agentSessionId);
    appendThinkingIndicator();
    setSessionActive(true);
    try {
      await invoke('agent_send_message', {
        sessionId: State.agentSessionId,
        text: fullText,
        background: true,
      });
      console.log('[chat] ✓ Message sent, restarting poll');
      startAgentPolling(State.agentSessionId);
    } catch (err) {
      console.error('[chat] ✗ Send failed:', err);
      removeThinkingIndicator();
      appendChatMessage({ kind: 'error', content: `Failed to send: ${err}`, ts: nowTs() });
    }
  } else {
    try {
      await invoke('send_message', { message: fullText, images: null });
    } catch (err) {
      appendChatMessage({ kind: 'error', content: `Failed to send: ${err}`, ts: nowTs() });
    }
  }
}

function autoResize() {
  this.style.height = 'auto';
  this.style.height = Math.min(this.scrollHeight, 120) + 'px';
}

// ── Restore session ──────────────────────────────────────────────────────────

async function restoreInitialSession() {
  try {
    const [hist, detail] = await Promise.all([
      invoke('get_session_history'),
      invoke('get_current_session'),
    ]);
    setText('sb-badge-sessions', hist?.length ? String(hist.length) : '');
    if (detail?.summary?.status === 'running') {
      renderSessionDetail(detail);
      navigate('home', { preserveHomeMode: true });
    } else {
      await showHomeLanding({ clearBackend: false });
    }
  } catch {
    await showHomeLanding({ clearBackend: false });
  }
}

// ── Boot ──────────────────────────────────────────────────────────────────────

init().catch(console.error);

// ── Developer bootstrap DOMContentLoaded wiring ──────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
  const devBtn = document.getElementById('engine-developer');
  if (devBtn) {
    devBtn.addEventListener('click', () => {
      hideEngineSetupModal();
      showDeveloperBootstrap();
    });
  }

  setupDeveloperBootstrapHandlers();
});
