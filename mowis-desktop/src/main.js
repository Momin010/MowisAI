/**
 * MowisAI Desktop — Minimal Chat-First Application
 * Tauri v2 + Vite. Graceful browser fallback.
 */

import { loadTauri, isTauri, invoke, listen, openDialogNative } from './bridge.js';
import { delay, nowTs, fmtNumber, fmtTs, escapeHtml, parseGitHubRepoName } from './utils.js';

// ── App State ─────────────────────────────────────────────────────────────────

const State = {
  page: 'home',
  homeMode: 'new',
  sessionActive: false,
  sessionId: null,
  taskPanelOpen: false,
  selectedTaskId: null,
  sidebarCollapsed: localStorage.getItem('mowis_sidebar_collapsed') === '1',
  tasks: {},
  streamingContent: '',
  isStreaming: false,
  daemonConnected: false,
  config: null,
  selectedRepo: null,
  cloneDestination: null,
  setupError: null,
  stats: { tasks_total: 0, tasks_done: 0, tasks_running: 0, tokens_total: 0, tool_calls: 0 },
  zeroWorkspacePath: null,  // set when a zero-mode session is active
  fileChanges: [],          // recent FileChange[] batches
  selectedChangePath: null, // selected file path for diff panel
  planCardShown: false,     // only show the first plan card, skip the rest
  diffTree: {
    query: '',
    actions: new Set(['created', 'modified', 'deleted', 'moved', 'read']),
    expanded: new Set(), // folder paths
  },
};

// ── Helpers ───────────────────────────────────────────────────────────────────

const $ = (id) => document.getElementById(id);
const setText = (id, v) => { const e = $(id); if (e) e.textContent = v; };

function toast(msg, type = 'info') {
  const c = $('toasts');
  if (!c) return;
  const t = document.createElement('div');
  t.className = `toast ${type}`;
  t.textContent = msg;
  c.appendChild(t);
  setTimeout(() => { t.style.opacity = '0'; t.style.transition = 'opacity 0.3s'; setTimeout(() => t.remove(), 320); }, 3200);
}

// ── Engine setup modal helpers ────────────────────────────────────────────────

function hideEngineSetupModal() {
  const modal = $('engine-setup-modal');
  if (!modal) return;
  modal.classList.add('hidden');
  modal.setAttribute('aria-hidden', 'true');
}

function setModeToZeroAndReflectUI() {
  if (!State.config) State.config = {};
  State.config.mode = 'zero';

  const homeMode = $('home-mode');
  if (homeMode) {
    homeMode.value = 'zero';
    updateHomeZeroHint('zero');
    syncCustomSelect(homeMode);
  }

  const setMode = $('set-mode');
  if (setMode) {
    setMode.value = 'zero';
    syncCustomSelect(setMode);
  }
}

async function pickPathWithDialog({ title, directory = false }) {
  try {
    const selected = await openDialogNative({
      title,
      multiple: false,
      directory,
    });
    if (!selected) return null;
    return Array.isArray(selected) ? selected[0] : selected;
  } catch {
    return null;
  }
}

function normalizeQemuPathFromSelection(selection) {
  if (!selection) return '';
  const normalized = String(selection).replace(/\//g, '\\');
  if (normalized.toLowerCase().endsWith('.exe')) return normalized;
  return `${normalized.replace(/[\\\/]+$/, '')}\\qemu-system-x86_64.exe`;
}

async function handlePointInstallationFlow() {
  const qemuSelection = await pickPathWithDialog({
    title: 'Select qemu-system-x86_64.exe or QEMU folder',
    directory: false,
  }) || await pickPathWithDialog({
    title: 'Select QEMU installation folder',
    directory: true,
  });

  if (!qemuSelection) {
    toast('Installation path selection cancelled', 'info');
    return;
  }

  const qemuPath = normalizeQemuPathFromSelection(qemuSelection);

  // Save only the QEMU path for the WHPX fallback launcher.
  // The full developer bootstrap (ISO + disk) is handled separately.
  const defaultCfg = {
    qemu_path: qemuPath,
    iso_path: '',
    disk_path: '',
    mount_point: '/mnt/mowisai',
    disk_device: '/dev/sda',
    ram_mb: 512,
    agent_port: 8080,
    monitor_port: 4445,
    serial_port: 4444,
    agentd_path: '/mnt/mowisai/agentd',
  };

  try {
    const existing = await invoke('get_developer_config');
    const cfg = { ...existing, qemu_path: qemuPath };

    await invoke('save_developer_config', { config: cfg });
    hideEngineSetupModal();
    toast('QEMU path saved. Retrying engine startup...', 'success');
    setTimeout(() => window.location.reload(), 350);
  } catch (e) {
    toast(`Could not save installation: ${e}`, 'error');
  }
}

function setupEngineSetupModalHandlers() {
  const retryBtn = $('engine-retry');
  if (retryBtn) {
    retryBtn.onclick = () => {
      hideEngineSetupModal();
      window.location.reload();
    };
  }

  const continueBtn = $('engine-continue');
  if (continueBtn) {
    continueBtn.onclick = async () => {
      hideEngineSetupModal();
      setModeToZeroAndReflectUI();
      try { await invoke('save_config', { config: State.config }); } catch {}
      toast('Continuing in Zero-Protection mode', 'success');
    };
  }

  const manualBtn = $('engine-manual');
  if (manualBtn) {
    manualBtn.onclick = async () => {
      toast('Point to your QEMU installation', 'info');
      await handlePointInstallationFlow();
    };
  }

  const helpWsl = $('engine-help-wsl');
  if (helpWsl) {
    helpWsl.onclick = (e) => {
      e.preventDefault();
      toast('Open PowerShell (Admin) and run: wsl --install', 'info');
    };
  }

  const helpQemu = $('engine-help-qemu');
  if (helpQemu) {
    helpQemu.onclick = (e) => {
      e.preventDefault();
      toast('Install QEMU, then restart the app', 'info');
    };
  }
}

// ── Navigation ────────────────────────────────────────────────────────────────

function navigate(page, opts = {}) {
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

async function showHomeLanding({ clearBackend = false } = {}) {
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
    State.sessionId = null;
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

function showSessionShell() {
  State.homeMode = 'session';
  $('home-empty')?.classList.add('hidden');
  $('home-chat')?.classList.remove('hidden');
  updateTaskPanelVisibility();
}

function renderSessionDetail(detail) {
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

async function openSession(sessionId) {
  if (State.sessionActive && State.sessionId && sessionId !== State.sessionId) {
    toast('Stop the running session before opening another one', 'error');
    return;
  }
  try {
    const detail = await invoke('load_session', { sessionId });
    renderSessionDetail(detail);
    navigate('home', { preserveHomeMode: true });
  } catch (err) {
    toast('Could not open session: ' + err, 'error');
  }
}

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

// ── Window controls (decorations: false) ─────────────────────────────────────

async function runWindowAction(action) {
  // Use the Tauri JS API directly — do NOT also call the custom window_control
  // command, as that would double-toggle (e.g. maximize then unmaximize).
  if (!isTauri()) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (action === 'close') await win.close();
    if (action === 'minimize') await win.minimize();
    if (action === 'toggle_maximize') await win.toggleMaximize();
  } catch {}
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

  // Forward real setup-progress events from BackendBridge while booting.
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

  // Show "Continue in background" button after 3 seconds
  const showContinueTimer = delay(3000).then(() => {
    if (continueBtn) continueBtn.classList.remove('hidden');
  });

  // "Continue in background" dismisses the splash early
  let earlyDismiss = null;
  if (continueBtn) {
    continueBtn.addEventListener('click', () => {
      if (earlyDismiss) earlyDismiss();
    }, { once: true });
  }

  // Give the backend up to 90 s (WSL install + QEMU can take a while).
  const result = await Promise.race([
    backendReady,
    delay(90000).then(() => ({ stage: 'timeout' })),
    new Promise(resolve => { earlyDismiss = resolve; }),
  ]);
  if (unlisten) unlisten();

  // Store any setup error so we can show it in the modal
  if (lastErrorMessage) State.setupError = lastErrorMessage;

  if (fill) fill.style.width = '100%';
  await delay(200);
  $('splash')?.classList.add('hidden');
  $('app')?.classList.remove('hidden');

  // If there was an error, show the engine setup modal
  if (result && (result.stage === 'error' || result.stage === 'timeout')) {
    const fullError = lastErrorDetail
      ? `${lastErrorMessage || 'Connection failed'}\n\n${lastErrorDetail}`
      : (lastErrorMessage || 'Connection timed out');
    showEngineSetupModal(fullError);
  }
}

function showEngineSetupModal(errorMessage) {
  const modal = $('engine-setup-modal');
  if (!modal) return;

  const statusMsg = $('engine-status-message');
  const errorBlock = $('engine-error');
  const errorMsg = $('engine-error-message');

  if (statusMsg) statusMsg.textContent = 'Could not detect WSL or QEMU on your system';
  if (errorMsg) errorMsg.textContent = errorMessage;
  if (errorBlock) errorBlock.classList.remove('hidden');

  modal.classList.remove('hidden');
  modal.setAttribute('aria-hidden', 'false');
}

// ── Init ──────────────────────────────────────────────────────────────────────

async function init() {
  await loadTauri();
  setupWindowControls();   // non-blocking, sets up dot handlers
  setupEngineSetupModalHandlers();
  await runSplash();

  // Load config
  try { State.config = await invoke('get_config'); } catch { State.config = { socket_path: '/tmp/agentd.sock', max_agents: 100, mode: 'auto', provider: 'gemini', model: 'gemini-2.0-flash', api_key: '', gcp_project: '', gcp_region: 'us-central1', gcp_service_account_key_path: '' }; }

  // System info
  try {
    const info = await invoke('get_system_info');
    setText('about-meta', `${info.os} · ${info.arch} · MowisAI v${info.version}`);
    setText('tl-version', `v${info.version}`);
  } catch {}

  // Welcome screen on first launch
  await maybeShowWelcome();

  // Check daemon — show guidance banner if offline
  await checkDaemonWithGuidance();

  // Setup event listeners — wrapped so a listener failure doesn't kill navigation
  try { await setupListeners(); } catch (e) { console.error('Listener setup failed:', e); }

  // Setup UI handlers
  setupHandlers();
  initCustomSelects();
  setSidebarCollapsed(State.sidebarCollapsed);
  await restoreInitialSession();

  // Statusbar provider
  setText('sb-provider', State.config?.provider || '—');

  // Keyboard shortcuts
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

  // Trigger blur-to-clear animation after a short pause
  await delay(200);
  word?.classList.add('clear');

  // Show continue button after blur settles
  await delay(2200);
  cont?.classList.add('visible');

  // Wait for continue click
  await new Promise(resolve => {
    btn?.addEventListener('click', resolve, { once: true });
    // Also let Enter key proceed
    document.addEventListener('keydown', function handler(e) {
      if (e.key === 'Enter' || e.key === ' ') {
        document.removeEventListener('keydown', handler);
        resolve();
      }
    });
  });

  // Fade out
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

  // Start polling loop
  setTimeout(checkDaemon, 8000);
}

const OFFLINE_GUIDANCE = {
  windows: `MowisAI needs a Linux engine to run. It will automatically install one via WSL2.\n\nIf setup is taking a while: open PowerShell as Administrator and run <code>wsl --install</code>, then restart the app.`,
  macos: `MowisAI needs QEMU to run the Linux engine.\n\nInstall it with: <code>brew install qemu</code>\n\nThen restart the app.`,
  linux: `The agentd daemon is not running.\n\nStart it with: <code>sudo agentd socket --path /tmp/agentd.sock</code>`,
  unknown: `The agent engine is not connected. Check the Settings tab to verify your socket path.`,
};

function showOfflineBanner(os, launcher) {
  if ($('offline-banner')) return; // already shown
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

    // Prefer showing diffs in the right panel instead of task inspector.
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
    // Finalize streaming
    finalizeStreaming();

    // Reload sessions
    try {
      const hist = await invoke('get_session_history');
      if (State.page === 'sessions') renderSessionsPage();
      // Update badge
      setText('sb-badge-sessions', String(hist.length));
    } catch {}

    // DON'T call setSessionActive(false) - this would disable the send button
    // In zero mode, the session should stay active for follow-up messages
    // In orchestration mode, the session naturally completes but user can still send new messages

    updateStatusBar();
    // Don't show "Session complete" toast - it's confusing in zero mode
  });
}

// ── Session start ─────────────────────────────────────────────────────────────

async function startSession(prompt, mode, repo = State.selectedRepo) {
  if (!prompt.trim()) { toast('Enter a task description', 'error'); return; }

  // Reset chat
  State.tasks = {};
  State.selectedTaskId = null;
  State.taskPanelOpen = false;
  State.isStreaming = false;
  State.streamingContent = '';
  State.planCardShown = false;

  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';

  const taskPanelBody = $('task-panel-body');
  if (taskPanelBody) taskPanelBody.innerHTML = '';

  showSessionShell();

  setSessionActive(true);

  // Show user message immediately
  appendChatMessage({ kind: 'user', content: prompt, ts: nowTs() });
  if (repo?.path) {
    appendChatMessage({
      kind: 'system',
      content: `Repository attached: ${repo.name || 'repository'} (${repo.path})`,
      ts: nowTs(),
    });
  }

  // Clear task panel
  updateTaskPanelVisibility();

  try {
    const id = await invoke('start_session', {
      prompt,
      mode: mode || 'auto',
      projectPath: repo?.path || null,
      repoUrl: repo?.repo_url || repo?.remote_url || null,
      repoSource: repo?.source || null,
    });
    State.sessionId = id;
    setText('compose-session-info', `session ${id.slice(0,12)}`);
    setText('chat-session-title', prompt.slice(0, 120));

    // If zero mode, fetch and display the workspace path.
    if ((mode || State.config?.mode) === 'zero') {
      await refreshZeroWorkspace();
    } else {
      updateZeroWorkspaceBar(null);
    }
  } catch (err) {
    appendChatMessage({ kind: 'error', content: String(err), ts: nowTs() });
    setSessionActive(false);
  }

  // Navigate to home
  navigate('home', { preserveHomeMode: true });
}

function setSessionActive(active, keepChat = false) {
  State.sessionActive = active;
  const stopBtn = $('btn-stop');
  const sendBtn = $('btn-chat-send');
  const homeBtn = $('btn-home-send');
  if (stopBtn) stopBtn.style.display = active ? '' : 'none';
  if (sendBtn) sendBtn.disabled = active;
  if (homeBtn) homeBtn.disabled = active;
}

// ── Chat rendering ────────────────────────────────────────────────────────────

function appendChatMessage(msg) {
  const container = $('chat-messages');
  if (!container) return;

  // Finalize any open streaming bubble first
  if (State.isStreaming && msg.kind !== 'agent_chunk') {
    finalizeStreaming();
  }

  let row;

  if (msg.kind === 'user') {
    row = createMessageRow('user', msg.content);
  } else if (msg.kind === 'agent') {
    row = createMessageRow('agent', msg.content);
    if (msg.streaming) State.isStreaming = true;
  } else if (msg.kind === 'system') {
    row = createMessageRow('system', msg.content);
  } else if (msg.kind === 'plan') {
    // Only show the first plan card; subsequent ones are clutter.
    if (!State.planCardShown) {
      State.planCardShown = true;
      row = createPlanCard(msg);
    }
  } else if (msg.kind === 'error') {
    row = createErrorCard(msg.content);
  }

  if (row) {
    container.appendChild(row);
    scrollToBottom(container);
  }
}

function appendAgentChunk(chunk) {
  const container = $('chat-messages');
  if (!container) return;

  if (!State.isStreaming) {
    // Open a new streaming bubble
    State.streamingContent = '';
    State.isStreaming = true;
    const row = document.createElement('div');
    row.className = 'msg-row agent';
    row.id = 'streaming-bubble';
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    bubble.id = 'streaming-text';
    const cursor = document.createElement('span');
    cursor.className = 'cursor';
    cursor.id = 'streaming-cursor';
    bubble.appendChild(cursor);
    row.appendChild(bubble);
    container.appendChild(row);
  }

  State.streamingContent += chunk;

  const textEl = $('streaming-text');
  const cursor = $('streaming-cursor');
  if (textEl) {
    // Render markdown-lite: bold, italic
    const html = mdLite(State.streamingContent);
    textEl.innerHTML = html;
    // Re-append cursor
    const cur = document.createElement('span');
    cur.className = 'cursor';
    cur.id = 'streaming-cursor';
    textEl.appendChild(cur);
  }

  scrollToBottom(container);
}

function finalizeStreaming() {
  if (!State.isStreaming) return;
  State.isStreaming = false;
  const cursor = $('streaming-cursor');
  if (cursor) cursor.remove();
  // Remove id from bubble
  const bubble = $('streaming-bubble');
  if (bubble) bubble.removeAttribute('id');
  const text = $('streaming-text');
  if (text) text.removeAttribute('id');
  State.streamingContent = '';
}

function appendFileChanges(changes) {
  const container = $('chat-messages');
  if (!container) return;

  // Finalize any streaming first
  if (State.isStreaming) {
    finalizeStreaming();
  }

  const row = document.createElement('div');
  row.className = 'msg-row file-changes';
  
  const card = document.createElement('div');
  card.className = 'file-changes-card';
  
  changes.forEach(change => {
    const item = document.createElement('div');
    item.className = 'file-change-item';
    item.dataset.action = change.action;
    item.title = change.path; // Tooltip shows full path
    
    // Icon based on action
    const icon = document.createElement('span');
    icon.className = 'file-icon';
    icon.innerHTML = getFileActionIcon(change.action);
    
    // Filename only (not full path)
    const filename = change.path.split('/').pop() || change.path;
    const label = document.createElement('span');
    label.className = 'file-label';
    label.textContent = filename;
    
    // Line count badge (show +X/-Y if available)
    if (change.lines_added > 0 || change.lines_deleted > 0) {
      const badge = document.createElement('span');
      badge.className = 'line-count-badge';
      const parts = [];
      if (change.lines_added > 0) parts.push(`+${change.lines_added}`);
      if (change.lines_deleted > 0) parts.push(`-${change.lines_deleted}`);
      badge.textContent = parts.join(' ');
      label.appendChild(badge);
    }
    
    item.appendChild(icon);
    item.appendChild(label);
    
    // Click to open diff viewer
    item.addEventListener('click', () => {
      openDiffViewer(change);
    });
    
    card.appendChild(item);
  });
  
  row.appendChild(card);
  container.appendChild(row);
  scrollToBottom(container);
}

// ── Diff Viewer ───────────────────────────────────────────────────────────────

function openDiffViewer(change) {
  // Create modal overlay
  const overlay = document.createElement('div');
  overlay.className = 'diff-viewer-overlay';
  
  const modal = document.createElement('div');
  modal.className = 'diff-viewer-modal';
  
  // Header
  const header = document.createElement('div');
  header.className = 'diff-viewer-header';
  
  const title = document.createElement('div');
  title.className = 'diff-viewer-title';
  title.textContent = change.path;
  
  const stats = document.createElement('div');
  stats.className = 'diff-viewer-stats';
  if (change.lines_added > 0 || change.lines_deleted > 0) {
    const parts = [];
    if (change.lines_added > 0) parts.push(`+${change.lines_added} added`);
    if (change.lines_deleted > 0) parts.push(`-${change.lines_deleted} deleted`);
    stats.textContent = parts.join(' • ');
  } else {
    stats.textContent = change.action;
  }
  
  const closeBtn = document.createElement('button');
  closeBtn.className = 'diff-viewer-close';
  closeBtn.innerHTML = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>`;
  closeBtn.onclick = () => overlay.remove();
  
  header.appendChild(title);
  header.appendChild(stats);
  header.appendChild(closeBtn);
  
  // Content area
  const content = document.createElement('div');
  content.className = 'diff-viewer-content';
  
  if (change.content) {
    // Show actual code with line numbers
    const lines = change.content.split('\n');
    const codeBlock = document.createElement('pre');
    codeBlock.className = 'diff-viewer-code';
    
    lines.forEach((line, idx) => {
      const lineDiv = document.createElement('div');
      lineDiv.className = 'code-line';
      
      const lineNum = document.createElement('span');
      lineNum.className = 'line-number';
      lineNum.textContent = idx + 1;
      
      const lineContent = document.createElement('span');
      lineContent.className = 'line-content';
      lineContent.textContent = line || ' '; // Empty lines need space
      
      lineDiv.appendChild(lineNum);
      lineDiv.appendChild(lineContent);
      codeBlock.appendChild(lineDiv);
    });
    
    content.appendChild(codeBlock);
  } else {
    // No content available
    const placeholder = document.createElement('div');
    placeholder.className = 'diff-viewer-placeholder';
    placeholder.textContent = change.action === 'deleted' 
      ? 'File was deleted' 
      : 'Content not available';
    content.appendChild(placeholder);
  }
  
  modal.appendChild(header);
  modal.appendChild(content);
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  
  // Close on overlay click
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) overlay.remove();
  });
  
  // Close on Escape key
  const escHandler = (e) => {
    if (e.key === 'Escape') {
      overlay.remove();
      document.removeEventListener('keydown', escHandler);
    }
  };
  document.addEventListener('keydown', escHandler);
}

// ── Diff Panel (right sidebar) ────────────────────────────────────────────────

function normalizeNewlines(text) {
  if (text == null) return '';
  let t = String(text);
  // Handle accidentally escaped newlines coming from some sources
  if (t.includes('\\n') && !t.includes('\n')) t = t.replace(/\\n/g, '\n');
  return t.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
}

function diffLines(beforeText, afterText) {
  const a = normalizeNewlines(beforeText).split('\n');
  const b = normalizeNewlines(afterText).split('\n');

  // Myers diff (line-based), good enough for UI.
  const N = a.length;
  const M = b.length;
  const max = N + M;
  const v = new Map();
  v.set(1, 0);
  const trace = [];

  for (let d = 0; d <= max; d++) {
    const vNext = new Map(v);
    for (let k = -d; k <= d; k += 2) {
      let x;
      if (k === -d || (k !== d && (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0))) {
        x = v.get(k + 1) ?? 0; // down
      } else {
        x = (v.get(k - 1) ?? 0) + 1; // right
      }
      let y = x - k;
      while (x < N && y < M && a[x] === b[y]) {
        x++;
        y++;
      }
      vNext.set(k, x);
      if (x >= N && y >= M) {
        trace.push(vNext);
        return backtrack(trace, a, b);
      }
    }
    trace.push(vNext);
    v.clear();
    for (const [k, x] of vNext) v.set(k, x);
  }

  return b.map(line => ({ type: 'ctx', text: line }));
}

function backtrack(trace, a, b) {
  let x = a.length;
  let y = b.length;
  const edits = [];

  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d];
    const k = x - y;
    let prevK;
    if (k === -d || (k !== d && (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0))) {
      prevK = k + 1;
    } else {
      prevK = k - 1;
    }
    const prevX = v.get(prevK) ?? 0;
    const prevY = prevX - prevK;

    while (x > prevX && y > prevY) {
      edits.push({ type: 'ctx', text: a[x - 1] });
      x--;
      y--;
    }

    if (d === 0) break;

    if (x === prevX) {
      // insertion
      edits.push({ type: 'add', text: b[y - 1] });
      y--;
    } else {
      // deletion
      edits.push({ type: 'del', text: a[x - 1] });
      x--;
    }
  }

  edits.reverse();
  return edits;
}

function latestFlattenedChanges() {
  const flat = [];
  for (const batch of State.fileChanges) {
    for (const c of batch.changes) flat.push(c);
  }
  // dedupe by path keeping newest first
  const seen = new Set();
  const out = [];
  for (const c of flat) {
    if (seen.has(c.path)) continue;
    seen.add(c.path);
    out.push(c);
  }
  return out;
}

function buildChangeTree(changes) {
  const root = { name: '', path: '', kind: 'dir', children: new Map() };
  for (const c of changes) {
    const parts = String(c.path || '').split('/').filter(Boolean);
    let cur = root;
    let curPath = '';
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      curPath = curPath ? `${curPath}/${part}` : part;
      const isLeaf = i === parts.length - 1;
      if (isLeaf) {
        cur.children.set(curPath, { name: part, path: curPath, kind: 'file', change: c });
      } else {
        if (!cur.children.has(curPath)) {
          cur.children.set(curPath, { name: part, path: curPath, kind: 'dir', children: new Map() });
        }
        cur = cur.children.get(curPath);
      }
    }
  }
  return root;
}

function treeToRows(node, depth, rows, opts) {
  const { query, actions, expanded } = opts;
  const kids = [...(node.children?.values() || [])];
  kids.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'dir' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  for (const child of kids) {
    if (child.kind === 'file') {
      const c = child.change;
      const okAction = actions.has(String(c.action));
      const okQuery = !query || child.path.toLowerCase().includes(query);
      if (!okAction || !okQuery) continue;
      rows.push({ kind: 'file', depth, path: c.path, name: child.name, action: c.action });
      continue;
    }

    const isOpen = expanded.has(child.path);
    const tmp = [];
    treeToRows(child, depth + 1, tmp, opts);
    if (tmp.length === 0) continue;

    rows.push({ kind: 'dir', depth, path: child.path, name: child.name, open: isOpen, count: tmp.filter(r => r.kind === 'file').length });
    if (isOpen) rows.push(...tmp);
  }
}

function renderDiffPanel() {
  const panel = $('task-panel');
  if (!panel) return;

  // Replace task inspector chrome
  const title = panel.querySelector('.task-panel-title');
  if (title) title.textContent = 'Diff';
  const subtitle = $('task-panel-subtitle');
  if (subtitle) subtitle.textContent = 'Select a file to inspect changes';

  const body = $('task-panel-body');
  const detail = $('task-detail');
  if (!body || !detail) return;

  const changes = latestFlattenedChanges();
  const tree = buildChangeTree(changes);
  const q = (State.diffTree?.query || '').trim().toLowerCase();
  const actions = State.diffTree?.actions || new Set(['created', 'modified', 'deleted', 'moved', 'read']);
  const expanded = State.diffTree?.expanded || new Set();
  const rows = [];
  treeToRows(tree, 0, rows, { query: q, actions, expanded });

  const actionBtn = (id, label) => {
    const on = actions.has(id);
    return `<button class="diff-filter ${on ? 'on' : ''}" data-action="${escHtml(id)}">${escHtml(label)}</button>`;
  };

  const controls = `
    <div class="diff-tree-controls">
      <input class="diff-tree-search" id="diff-tree-search" placeholder="Search files…" value="${escHtml(State.diffTree?.query || '')}" />
      <div class="diff-filter-row">
        ${actionBtn('created','Created')}
        ${actionBtn('modified','Modified')}
        ${actionBtn('deleted','Deleted')}
        ${actionBtn('moved','Moved')}
        ${actionBtn('read','Read')}
        <button class="diff-filter ghost" id="diff-filter-all">All</button>
        <button class="diff-filter ghost" id="diff-filter-none">None</button>
        <button class="diff-filter ghost" id="diff-collapse-all">Collapse</button>
      </div>
    </div>
  `;

  const rowsHtml = changes.length === 0
    ? `<div class="task-row"><div class="task-desc">No file changes yet</div></div>`
    : rows.map(r => {
        const pad = `style="padding-left:${12 + r.depth * 14}px"`;
        if (r.kind === 'dir') {
          return `
            <div class="diff-tree-row dir" data-dir="${escHtml(r.path)}" ${pad}>
              <span class="chev">${r.open ? '▾' : '▸'}</span>
              <span class="name">${escHtml(r.name)}</span>
              <span class="meta">${r.count}</span>
            </div>
          `;
        }
        const selected = State.selectedChangePath === r.path ? 'selected' : '';
        return `
          <div class="diff-tree-row file ${selected}" data-path="${escHtml(r.path)}" ${pad}>
            <span class="task-dot ${r.action}"></span>
            <span class="name">${escHtml(r.name)}</span>
            <span class="meta">${escHtml(r.action)}</span>
          </div>
        `;
      }).join('');

  body.innerHTML = controls + `<div class="diff-tree-rows">${rowsHtml}</div>`;

  $('diff-tree-search')?.addEventListener('input', (e) => {
    State.diffTree.query = e.target.value || '';
    renderDiffPanel();
  });
  body.querySelectorAll('.diff-filter[data-action]').forEach(btn => {
    btn.addEventListener('click', () => {
      const a = btn.dataset.action;
      if (!a) return;
      if (State.diffTree.actions.has(a)) State.diffTree.actions.delete(a);
      else State.diffTree.actions.add(a);
      renderDiffPanel();
    });
  });
  $('diff-filter-all')?.addEventListener('click', () => {
    State.diffTree.actions = new Set(['created', 'modified', 'deleted', 'moved', 'read']);
    renderDiffPanel();
  });
  $('diff-filter-none')?.addEventListener('click', () => {
    State.diffTree.actions = new Set();
    renderDiffPanel();
  });
  $('diff-collapse-all')?.addEventListener('click', () => {
    State.diffTree.expanded = new Set();
    renderDiffPanel();
  });

  body.querySelectorAll('.diff-tree-row.dir').forEach(row => {
    row.addEventListener('click', () => {
      const d = row.dataset.dir;
      if (!d) return;
      if (State.diffTree.expanded.has(d)) State.diffTree.expanded.delete(d);
      else State.diffTree.expanded.add(d);
      renderDiffPanel();
    });
  });
  body.querySelectorAll('.diff-tree-row.file').forEach(row => {
    row.addEventListener('click', () => {
      const p = row.dataset.path;
      if (!p) return;
      State.selectedChangePath = p;
      renderDiffPanel();
    });
  });

  const selected = changes.find(c => c.path === State.selectedChangePath) || changes[0] || null;
  if (!State.selectedChangePath && selected) State.selectedChangePath = selected.path;

  if (!selected) {
    detail.innerHTML = `<div class="task-detail-empty">No diff to show yet.</div>`;
    return;
  }

  // Diff view
  const before = selected.before_content ?? '';
  const after = selected.content ?? '';

  let hunks = [];
  if (selected.action === 'created') {
    hunks = normalizeNewlines(after).split('\n').map(t => ({ type: 'add', text: t }));
  } else if (selected.action === 'deleted') {
    hunks = normalizeNewlines(before).split('\n').map(t => ({ type: 'del', text: t }));
  } else if (selected.action === 'modified') {
    hunks = diffLines(before, after);
  } else {
    // read/moved fallback
    hunks = normalizeNewlines(after || before).split('\n').map(t => ({ type: 'ctx', text: t }));
  }

  const header = `
    <div class="diff-panel-head">
      <div class="diff-panel-path">${escHtml(selected.path)}</div>
      <div class="diff-panel-meta">${escHtml(selected.action)}</div>
    </div>
  `;

  let addCount = 0, delCount = 0;
  for (const h of hunks) { if (h.type === 'add') addCount++; if (h.type === 'del') delCount++; }
  if (subtitle) subtitle.textContent = `${addCount} added · ${delCount} removed`;

  const linesHtml = hunks.map((h, i) => {
    const sign = h.type === 'add' ? '+' : h.type === 'del' ? '-' : ' ';
    return `
      <div class="diff-line ${h.type}">
        <span class="diff-gutter">${sign}</span>
        <span class="diff-lno">${i + 1}</span>
        <span class="diff-text">${escHtml(h.text || ' ')}</span>
      </div>
    `;
  }).join('');

  detail.innerHTML = `${header}<div class="diff-panel-body">${linesHtml || '<div class="task-detail-empty">Empty file</div>'}</div>`;
}

function getFileActionIcon(action) {
  // Using SVG icons (Lucide-style)
  const icons = {
    created: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="12" y1="18" x2="12" y2="12"></line><line x1="9" y1="15" x2="15" y2="15"></line></svg>`,
    modified: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><path d="M10.4 12.6a2 2 0 1 1 3 3L8 21l-4 1 1-4Z"></path></svg>`,
    deleted: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="9" y1="15" x2="15" y2="15"></line></svg>`,
    read: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><path d="M16 13H8"></path><path d="M16 17H8"></path><path d="M10 9H8"></path></svg>`,
    moved: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><polyline points="10 9 9 9 8 9"></polyline><path d="m15 14-3-3 3-3"></path></svg>`,
  };
  return icons[action] || icons.modified;
}

function createMessageRow(type, content) {
  const row = document.createElement('div');
  row.className = `msg-row ${type}`;
  const bubble = document.createElement('div');
  bubble.className = 'msg-bubble';
  bubble.innerHTML = type === 'agent' ? mdLite(content) : escHtml(content);
  row.appendChild(bubble);
  return row;
}

function createPlanCard(msg) {
  const row = document.createElement('div');
  row.className = 'msg-row plan';
  row.style.padding = '0 40px';

  const card = document.createElement('div');
  card.className = 'plan-card';
  card.innerHTML = `
    <div class="plan-card-title">▶ Orchestration Plan</div>
    <div class="plan-card-row">
      <div class="plan-stat">
        <div class="plan-stat-val">${msg.task_count}</div>
        <div class="plan-stat-lbl">Tasks</div>
      </div>
      <div class="plan-stat">
        <div class="plan-stat-val">${msg.agent_count}</div>
        <div class="plan-stat-lbl">Agents</div>
      </div>
      <div class="plan-stat">
        <div class="plan-stat-val">${(msg.mode || 'auto').toUpperCase()}</div>
        <div class="plan-stat-lbl">Mode</div>
      </div>
    </div>
    <div class="plan-sandboxes">
      ${(msg.sandboxes || []).map(s => `<span class="plan-sb${s === 'zero' ? ' plan-sb-zero' : ''}">${s === 'zero' ? '✦ ' : ''}${s}</span>`).join('')}
    </div>
  `;
  card.addEventListener('click', () => setTaskPanelOpen(true));
  row.appendChild(card);
  return row;
}

function createErrorCard(content) {
  const row = document.createElement('div');
  row.className = 'msg-row system';
  row.style.padding = '4px 40px';
  const card = document.createElement('div');
  card.className = 'error-card';
  card.textContent = content;
  row.appendChild(card);
  return row;
}

function scrollToBottom(el) {
  requestAnimationFrame(() => { el.scrollTop = el.scrollHeight; });
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
  // If we have file changes, the right panel becomes a Diff panel.
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
  // Diff panel owns the detail area when changes exist.
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

// ── Status bar (removed) ────────────────────────────────────────────────────

function updateStatusBar() {
  // Status bar removed — no-op
}

// ── Sessions page ─────────────────────────────────────────────────────────────

const SessionsState = {
  all: [],
  search: '',
  filter: 'all',
  sort: 'newest',
};

function relativeTime(ts) {
  if (!ts) return '';
  const now = Date.now();
  const diff = now - ts * 1000;
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return 'just now';
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

function formatDuration(secs) {
  if (!secs || secs <= 0) return null;
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const s = secs % 60;
  if (mins < 60) return s > 0 ? `${mins}m ${s}s` : `${mins}m`;
  const hrs = Math.floor(mins / 60);
  const m = mins % 60;
  return m > 0 ? `${hrs}h ${m}m` : `${hrs}h`;
}

function sessionsMatchFilter(s, filter) {
  if (filter === 'all') return true;
  return s.status === filter;
}

function sessionsMatchSearch(s, query) {
  if (!query) return true;
  const q = query.toLowerCase();
  return (s.prompt || '').toLowerCase().includes(q)
    || (s.id || '').toLowerCase().includes(q)
    || relativeTime(s.started_at).toLowerCase().includes(q);
}

function sessionsSort(a, b, sort) {
  switch (sort) {
    case 'oldest': return a.started_at - b.started_at;
    case 'tokens': return (b.tokens_total || 0) - (a.tokens_total || 0);
    case 'duration': return (b.duration_secs || 0) - (a.duration_secs || 0);
    case 'newest':
    default: return b.started_at - a.started_at;
  }
}

function renderSessionCard(s) {
  const progressPct = s.task_count > 0 ? Math.round((s.tasks_done / s.task_count) * 100) : 0;
  const progressClass = s.status === 'error' ? 'error' : s.status === 'done' ? 'complete' : '';
  const duration = formatDuration(s.duration_secs);
  const relative = relativeTime(s.started_at);
  const statusLabel = s.status === 'done' ? 'COMPLETED' : s.status.toUpperCase();

  return `
    <div class="session-card" data-id="${s.id}">
      <div class="sc-top">
        <div class="sc-prompt">${escHtml(s.prompt || '—')}</div>
        <div class="sc-badges">
          <span class="sc-status ${s.status}">${statusLabel}</span>
          ${s.mode ? `<span class="sc-mode">${escHtml(s.mode)}</span>` : ''}
        </div>
      </div>
      ${s.task_count > 0 ? `
        <div class="sc-progress-wrap">
          <div class="sc-progress-bar"><div class="sc-progress-fill ${progressClass}" style="width:${progressPct}%"></div></div>
          <div class="sc-progress-text">${s.tasks_done}/${s.task_count}</div>
        </div>
      ` : ''}
      <div class="sc-meta">
        ${(s.tokens_total || 0) > 0 ? `
          <span class="sc-meta-item">
            <svg width="11" height="11" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3"/><path d="M5 8h6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
            ${fmtNumber(s.tokens_total)} tokens
          </span>
          <span class="sc-meta-sep"></span>
        ` : ''}
        ${duration ? `
          <span class="sc-meta-item">
            <svg width="11" height="11" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3"/><path d="M8 5v3.5l2.5 1.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>
            ${duration}
          </span>
          <span class="sc-meta-sep"></span>
        ` : ''}
        <span class="sc-meta-item">
          <svg width="11" height="11" viewBox="0 0 16 16" fill="none"><rect x="2" y="3" width="12" height="10" rx="1.5" stroke="currentColor" stroke-width="1.3"/><path d="M2 6h12" stroke="currentColor" stroke-width="1.3"/></svg>
          ${relative}
        </span>
      </div>
    </div>`;
}

async function renderSessionsPage() {
  try {
    const hist = await invoke('get_session_history');
    SessionsState.all = hist || [];

    const list = $('sessions-list');
    const empty = $('sessions-empty');
    const toolbar = $('sessions-toolbar');
    const active = $('sessions-active');
    const noResults = $('sessions-no-results');

    setText('sb-badge-sessions', hist.length ? String(hist.length) : '');

    if (!hist.length) {
      if (empty) empty.style.display = '';
      if (list) list.style.display = 'none';
      if (toolbar) toolbar.style.display = 'none';
      if (active) active.style.display = 'none';
      if (noResults) noResults.style.display = 'none';
      return;
    }

    if (empty) empty.style.display = 'none';
    if (toolbar) toolbar.style.display = '';
    setText('sessions-count', `${hist.length} session${hist.length !== 1 ? 's' : ''}`);

    renderSessionsActive(active);
    renderSessionsList();
  } catch (e) {
    console.error(e);
  }
}

function renderSessionsActive(activeEl) {
  if (!activeEl) return;
  const running = SessionsState.all.find(s => s.status === 'running');
  if (!running) {
    activeEl.style.display = 'none';
    return;
  }
  activeEl.style.display = '';
  const progressPct = running.task_count > 0 ? Math.round((running.tasks_done / running.task_count) * 100) : 0;
  const relative = relativeTime(running.started_at);
  activeEl.innerHTML = `
    <div class="sessions-active-card" data-id="${running.id}">
      <div class="sessions-active-label">Currently running</div>
      <div class="sc-prompt">${escHtml(running.prompt || '—')}</div>
      <div class="sc-meta" style="margin-top:8px">
        ${running.task_count > 0 ? `
          <span class="sc-meta-item">${running.tasks_done}/${running.task_count} tasks</span>
          <span class="sc-meta-sep"></span>
        ` : ''}
        <span class="sc-meta-item">${relative}</span>
      </div>
    </div>`;
  activeEl.querySelector('.sessions-active-card').addEventListener('click', () => openSession(running.id));
}

function renderSessionsList() {
  const list = $('sessions-list');
  const noResults = $('sessions-no-results');
  if (!list) return;

  const filtered = SessionsState.all
    .filter(s => {
      if (s.status === 'running') return false;
      return sessionsMatchFilter(s, SessionsState.filter) && sessionsMatchSearch(s, SessionsState.search);
    })
    .sort((a, b) => sessionsSort(a, b, SessionsState.sort));

  if (!filtered.length) {
    list.style.display = 'none';
    if (noResults) noResults.style.display = '';
    return;
  }

  if (noResults) noResults.style.display = 'none';
  list.style.display = '';
  list.innerHTML = filtered.map(s => renderSessionCard(s)).join('');

  list.querySelectorAll('.session-card').forEach(card => {
    card.addEventListener('click', () => openSession(card.dataset.id));
  });
}

function setupSessionsHandlers() {
  // Search
  const searchInput = $('sessions-search');
  const searchClear = $('sessions-search-clear');
  if (searchInput) {
    searchInput.addEventListener('input', () => {
      SessionsState.search = searchInput.value.trim();
      if (searchClear) searchClear.classList.toggle('hidden', !SessionsState.search);
      renderSessionsList();
    });
  }
  if (searchClear) {
    searchClear.addEventListener('click', () => {
      SessionsState.search = '';
      if (searchInput) searchInput.value = '';
      searchClear.classList.add('hidden');
      renderSessionsList();
      if (searchInput) searchInput.focus();
    });
  }

  // Filter tabs
  document.querySelectorAll('.sessions-filter').forEach(btn => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.sessions-filter').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      SessionsState.filter = btn.dataset.filter;
      renderSessionsList();
    });
  });

  // Sort
  const sortSelect = $('sessions-sort');
  if (sortSelect) {
    sortSelect.addEventListener('change', () => {
      SessionsState.sort = sortSelect.value;
      renderSessionsList();
    });
  }

  // Empty state button
  $('btn-empty-new')?.addEventListener('click', () => navigate('home'));
}

// ── Usage page ────────────────────────────────────────────────────────────────

function fmtTokens(n) {
  if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(1) + 'B';
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
  return String(n);
}

function fmtDurationSecs(s) {
  if (!s || s <= 0) return '0m';
  if (s < 60) return s + 's';
  if (s < 3600) return Math.floor(s / 60) + 'm ' + (s % 60 > 0 ? (s % 60) + 's' : '');
  return Math.floor(s / 3600) + 'h ' + Math.floor((s % 3600) / 60) + 'm';
}

function smoothPath(points) {
  if (points.length < 2) return points.map(p => `M${p.x},${p.y}`).join('');
  let d = `M${points[0].x},${points[0].y}`;
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[Math.max(0, i - 1)];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[Math.min(points.length - 1, i + 2)];
    const cp1x = p1.x + (p2.x - p0.x) / 6;
    const cp1y = p1.y + (p2.y - p0.y) / 6;
    const cp2x = p2.x - (p3.x - p1.x) / 6;
    const cp2y = p2.y - (p3.y - p1.y) / 6;
    d += ` C${cp1x},${cp1y} ${cp2x},${cp2y} ${p2.x},${p2.y}`;
  }
  return d;
}

async function renderUsagePage() {
  try {
    const [stats, hist, usage] = await Promise.all([
      invoke('get_stats'),
      invoke('get_session_history'),
      invoke('get_usage_history'),
    ]);

    setText('us-sessions', hist.length);
    setText('us-tasks', fmtNumber(stats.lifetime_tasks ?? (stats.tasks_done + stats.tasks_total)));
    setText('us-tokens', fmtTokens(stats.lifetime_tokens ?? stats.tokens_total));
    setText('us-tools', fmtNumber(stats.lifetime_tool_calls ?? stats.tool_calls));

    const totalDuration = (usage || []).reduce((s, u) => s + (u.duration_secs || 0), 0);
    setText('us-duration', fmtDurationSecs(totalDuration));

    renderUsageChart(usage, stats);
    renderDonutChart(stats);
    renderSuccessRate(hist);
    renderTimeline(hist);
    renderUsageTable(hist);
  } catch (e) {
    console.error('Usage page error:', e);
  }
}

function renderUsageChart(usage, stats) {
  const el = $('usage-chart');
  if (!el) return;
  const rows = [...(usage || [])];
  const totalTokens = stats?.lifetime_tokens ?? rows.reduce((sum, u) => sum + (u.tokens || 0), 0);
  setText('usage-total-label', `${fmtTokens(totalTokens)} tokens`);

  if (!rows.length) {
    el.innerHTML = `<svg viewBox="0 0 640 200" role="img"><text x="280" y="105" fill="rgba(255,255,255,0.2)" font-size="12" font-family="var(--sans)">No usage recorded yet</text></svg>`;
    return;
  }

  const W = 640, H = 200, pad = 40, padR = 20;
  const maxVal = Math.max(...rows.map(r => r.tokens || 0), 1);
  const xStep = rows.length === 1 ? 0 : (W - pad - padR) / (rows.length - 1);
  const pts = rows.map((r, i) => ({
    x: rows.length === 1 ? W / 2 : pad + i * xStep,
    y: H - pad - ((r.tokens || 0) / maxVal) * (H - pad - 20),
    item: r,
  }));
  const lineD = smoothPath(pts);
  const areaD = lineD + ` L${pts[pts.length - 1].x},${H - pad} L${pts[0].x},${H - pad} Z`;

  // Y-axis labels
  const yTicks = 4;
  let gridLines = '';
  for (let i = 0; i <= yTicks; i++) {
    const y = 20 + ((H - pad - 20) / yTicks) * i;
    const val = maxVal * (1 - i / yTicks);
    gridLines += `<line x1="${pad}" y1="${y}" x2="${W - padR}" y2="${y}" stroke="rgba(255,255,255,0.04)" stroke-width="1"/>`;
    gridLines += `<text x="${pad - 6}" y="${y + 3}" text-anchor="end" fill="rgba(255,255,255,0.2)" font-size="9" font-family="var(--mono)">${fmtTokens(Math.round(val))}</text>`;
  }

  // X-axis labels (session numbers)
  let xLabels = '';
  const labelInterval = Math.max(1, Math.floor(rows.length / 8));
  pts.forEach((p, i) => {
    if (i % labelInterval === 0 || i === pts.length - 1) {
      xLabels += `<text x="${p.x}" y="${H - 10}" text-anchor="middle" fill="rgba(255,255,255,0.2)" font-size="9" font-family="var(--mono)">${i + 1}</text>`;
    }
  });

  el.innerHTML = `<svg viewBox="0 0 ${W} ${H}" role="img">
    <defs>
      <linearGradient id="chart-fill" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="rgba(59,139,255,0.25)"/>
        <stop offset="100%" stop-color="rgba(59,139,255,0)"/>
      </linearGradient>
    </defs>
    ${gridLines}
    <path d="${areaD}" fill="url(#chart-fill)"/>
    <path d="${lineD}" fill="none" stroke="var(--blue)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
    ${pts.map((p, i) => `<circle cx="${p.x}" cy="${p.y}" r="3" fill="var(--blue)" opacity="0.8"/>`).join('')}
    ${xLabels}
  </svg>`;
}

function renderDonutChart(stats) {
  const svg = $('usage-donut');
  const legend = $('usage-donut-legend');
  if (!svg || !legend) return;

  const total = stats?.lifetime_tokens ?? stats?.tokens_total ?? 0;
  if (total === 0) {
    svg.innerHTML = `<text x="100" y="105" text-anchor="middle" fill="rgba(255,255,255,0.2)" font-size="11">No data</text>`;
    legend.innerHTML = '';
    return;
  }

  const segments = [
    { label: 'Code Generation', pct: 0.32, color: '#3b8bff' },
    { label: 'Planning', pct: 0.22, color: '#a78bfa' },
    { label: 'Tool Calls', pct: 0.20, color: '#3ecf72' },
    { label: 'Code Review', pct: 0.15, color: '#e8be4a' },
    { label: 'Thinking', pct: 0.11, color: '#f05050' },
  ];

  const cx = 100, cy = 90, r = 65, sw = 18;
  const circ = 2 * Math.PI * r;
  let offset = 0;
  let arcs = '';

  segments.forEach(seg => {
    const len = seg.pct * circ;
    const gap = 3;
    arcs += `<circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="${seg.color}" stroke-width="${sw}" stroke-dasharray="${Math.max(0, len - gap)} ${circ - Math.max(0, len - gap)}" stroke-dashoffset="${-offset}" stroke-linecap="round" opacity="0.85"/>`;
    offset += len;
  });

  svg.innerHTML = arcs + `<text x="${cx}" y="${cy - 4}" text-anchor="middle" fill="var(--tx)" font-family="var(--serif)" font-size="20">${fmtTokens(total)}</text><text x="${cx}" y="${cy + 12}" text-anchor="middle" fill="var(--tx-4)" font-size="9" font-family="var(--sans)">total tokens</text>`;

  legend.innerHTML = segments.map(s => `<div class="usage-donut-item"><span class="usage-donut-dot" style="background:${s.color}"></span><span class="usage-donut-label">${s.label}</span><span class="usage-donut-pct">${Math.round(s.pct * 100)}%</span></div>`).join('');
}

function renderSuccessRate(hist) {
  const el = $('usage-success-rate');
  if (!el) return;
  if (!hist.length) { el.innerHTML = '<div class="usage-rate-empty">No data</div>'; return; }

  const done = hist.filter(s => s.status === 'done' || s.status === 'complete').length;
  const failed = hist.filter(s => s.status === 'error' || s.status === 'failed').length;
  const stopped = hist.filter(s => s.status === 'stopped').length;
  const total = hist.length;
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;

  el.innerHTML = `
    <div class="usage-rate-ring-wrap">
      <svg class="usage-rate-ring" viewBox="0 0 120 120">
        <circle cx="60" cy="60" r="48" fill="none" stroke="rgba(255,255,255,0.04)" stroke-width="10"/>
        <circle cx="60" cy="60" r="48" fill="none" stroke="${pct >= 80 ? 'var(--green)' : pct >= 50 ? 'var(--yellow)' : 'var(--red)'}" stroke-width="10" stroke-dasharray="${(pct / 100) * 301.6} ${301.6}" stroke-linecap="round" transform="rotate(-90 60 60)"/>
        <text x="60" y="56" text-anchor="middle" fill="var(--tx)" font-family="var(--serif)" font-size="24">${pct}%</text>
        <text x="60" y="72" text-anchor="middle" fill="var(--tx-4)" font-size="9" font-family="var(--sans)">success</text>
      </svg>
    </div>
    <div class="usage-rate-bars">
      <div class="usage-rate-bar-row"><span class="usage-rate-dot" style="background:var(--green)"></span><span>Completed</span><span class="usage-rate-count">${done}</span></div>
      <div class="usage-rate-bar-row"><span class="usage-rate-dot" style="background:var(--red)"></span><span>Failed</span><span class="usage-rate-count">${failed}</span></div>
      <div class="usage-rate-bar-row"><span class="usage-rate-dot" style="background:var(--yellow)"></span><span>Stopped</span><span class="usage-rate-count">${stopped}</span></div>
    </div>`;
}

function renderTimeline(hist) {
  const el = $('usage-timeline');
  if (!el) return;
  if (!hist.length) { el.innerHTML = '<div class="usage-rate-empty">No sessions</div>'; return; }

  const sorted = [...hist].sort((a, b) => a.started_at - b.started_at);
  const maxTokens = Math.max(...sorted.map(s => s.tokens || 0), 1);

  el.innerHTML = `<div class="usage-timeline-track">${sorted.map((s, i) => {
    const h = Math.max(8, ((s.tokens || 0) / maxTokens) * 60);
    const color = s.status === 'done' || s.status === 'complete' ? 'var(--green)' : s.status === 'error' || s.status === 'failed' ? 'var(--red)' : s.status === 'running' ? 'var(--blue)' : 'var(--yellow)';
    return `<div class="usage-timeline-bar" style="height:${h}px;background:${color}" title="${escHtml(s.prompt?.slice(0, 40) || 'Session')} — ${fmtTokens(s.tokens || 0)} tokens"></div>`;
  }).join('')}</div>`;
}

function renderUsageTable(hist) {
  const wrap = $('usage-sessions-table');
  if (!wrap) return;
  if (!hist.length) {
    wrap.innerHTML = '<div class="empty-state small"><div class="empty-text">No history yet</div></div>';
    return;
  }
  wrap.innerHTML = `<table class="usage-table">
    <thead><tr><th>Prompt</th><th>Status</th><th>Tasks</th><th>Tokens</th><th>Duration</th><th>Started</th></tr></thead>
    <tbody>${[...hist].reverse().map(s => `<tr>
      <td class="tx">${escHtml((s.prompt || '').slice(0, 50))}${(s.prompt || '').length > 50 ? '…' : ''}</td>
      <td><span class="sc-status ${s.status}">${s.status}</span></td>
      <td>${s.tasks_done || 0}/${s.task_count || 0}</td>
      <td>${fmtTokens(s.tokens || 0)}</td>
      <td>${fmtDurationSecs(s.duration_secs || 0)}</td>
      <td>${fmtTs(s.started_at)}</td>
    </tr>`).join('')}</tbody>
  </table>`;
}

function loadSettings() {
  const c = State.config || {};
  setVal('set-provider', c.provider || 'gemini');
  setVal('set-model', c.model || '');
  setVal('set-api-key', c.api_key || '');
  setVal('set-gcp', c.gcp_project || '');
  setVal('set-gcp-region', c.gcp_region || 'us-central1');
  setVal('set-sa-key', c.gcp_service_account_key_path || '');
  setVal('set-socket', c.socket_path || '/tmp/agentd.sock');
  setVal('set-mode', c.mode || 'auto');
  setVal('set-max-agents', c.max_agents || 100);
  const rowGcp = $('row-gcp');
  const rowGcpRegion = $('row-gcp-region');
  const rowSaKey = $('row-sa-key');
  const showVertex = (c.provider === 'vertex');
  if (rowGcp) rowGcp.style.display = showVertex ? '' : 'none';
  if (rowGcpRegion) rowGcpRegion.style.display = showVertex ? '' : 'none';
  if (rowSaKey) rowSaKey.style.display = showVertex ? '' : 'none';

  // Sandbox toggle
  const sandboxEnabled = c.sandbox_enabled !== false; // default true
  setVal('set-sandbox-enabled', sandboxEnabled);
  updateSandboxToggleLabel(sandboxEnabled);

  // Zero mode hint in settings
  updateSettingsZeroHint(c.mode || 'auto');

  // Sandbox status
  refreshSandboxInfo();
}

function setVal(id, val) {
  const e = $(id);
  if (!e) return;
  if (e.type === 'checkbox') e.checked = !!val;
  else e.value = val ?? '';
  if (e.tagName === 'SELECT') syncCustomSelect(e);
}
function getVal(id) { const e = $(id); if (!e) return ''; if (e.type === 'checkbox') return e.checked; return e.value; }

async function saveSettings() {
  const config = {
    socket_path: getVal('set-socket'),
    max_agents: parseInt(getVal('set-max-agents') || '100'),
    mode: getVal('set-mode'),
    provider: getVal('set-provider'),
    model: getVal('set-model'),
    api_key: getVal('set-api-key'),
    gcp_project: getVal('set-gcp'),
    gcp_region: getVal('set-gcp-region'),
    gcp_service_account_key_path: getVal('set-sa-key'),
    sandbox_enabled: getVal('set-sandbox-enabled'),
  };
  try {
    await invoke('save_config', { config });
    State.config = config;
    setText('sb-provider', config.provider);
    toast('Settings saved', 'success');
  } catch (err) {
    toast('Save failed: ' + err, 'error');
  }
}

// ── Zero mode helpers ─────────────────────────────────────────────────────────

function isZeroMode() {
  return (State.config?.mode || '') === 'zero';
}

function updateZeroWorkspaceBar(path) {
  State.zeroWorkspacePath = path || null;
  const bar = $('zero-workspace-bar');
  const pathEl = $('zero-workspace-path');
  if (!bar) return;
  if (path) {
    bar.classList.remove('hidden');
    if (pathEl) pathEl.textContent = path;
  } else {
    bar.classList.add('hidden');
  }
}

async function refreshZeroWorkspace() {
  try {
    const ws = await invoke('get_zero_workspace');
    updateZeroWorkspaceBar(ws?.path || null);
  } catch {}
}

// ── Sandbox helpers ───────────────────────────────────────────────────────────

function updateSettingsZeroHint(mode) {
  const hint = $('settings-zero-hint');
  if (!hint) return;
  if (mode === 'zero') {
    hint.classList.remove('hidden');
    // Populate base dir lazily.
    const base = $('zero-workspace-base');
    if (base && base.textContent === '…') {
      invoke('get_zero_workspace_base').then(p => { if (base) base.textContent = p; }).catch(() => {});
    }
  } else {
    hint.classList.add('hidden');
  }
}

function updateHomeZeroHint(mode) {
  const hint = $('home-zero-hint');
  if (!hint) return;
  hint.classList.toggle('hidden', mode !== 'zero');
}

function updateSandboxToggleLabel(enabled) {
  const label = $('sandbox-toggle-label');
  if (label) label.textContent = enabled ? 'On' : 'Off';
}

async function refreshSandboxInfo() {
  try {
    const info = await invoke('get_sandbox_status');
    const row = $('row-sandbox-info');
    const block = $('sandbox-info-block');
    const sizeEl = $('sandbox-size');
    if (!row || !block) return;

    if (!info) {
      row.style.display = 'none';
      return;
    }

    row.style.display = '';
    block.innerHTML = `
      <div class="sandbox-dir-row"><span class="sandbox-dir-label">lower</span><code class="sandbox-dir-path">${escHtml(info.lower_dir)}</code></div>
      <div class="sandbox-dir-row"><span class="sandbox-dir-label">upper</span><code class="sandbox-dir-path">${escHtml(info.upper_dir)}</code></div>
    `;

    // Fetch size asynchronously (best-effort)
    try {
      const bytes = await invoke('get_sandbox_size');
      if (sizeEl) sizeEl.textContent = bytes > 0 ? fmtBytes(bytes) : '';
    } catch {}
  } catch {}
}

function fmtBytes(bytes) {
  if (bytes >= 1_073_741_824) return (bytes / 1_073_741_824).toFixed(1) + ' GB';
  if (bytes >= 1_048_576)     return (bytes / 1_048_576).toFixed(1) + ' MB';
  if (bytes >= 1_024)         return (bytes / 1_024).toFixed(1) + ' KB';
  return bytes + ' B';
}

// ── Handlers ──────────────────────────────────────────────────────────────────

function setSidebarCollapsed(collapsed) {
  State.sidebarCollapsed = !!collapsed;
  document.querySelector('.layout')?.classList.toggle('sidebar-collapsed', State.sidebarCollapsed);
  localStorage.setItem('mowis_sidebar_collapsed', State.sidebarCollapsed ? '1' : '0');
  const btn = $('btn-sidebar-toggle');
  if (btn) {
    btn.title = State.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar';
    btn.setAttribute('aria-label', btn.title);
  }
}

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

function syncCustomSelect(selectOrId) {
  const select = typeof selectOrId === 'string' ? $(selectOrId) : selectOrId;
  if (!select) return;
  const wrap = document.querySelector(`.custom-select[data-select-id="${select.id}"]`);
  if (!wrap) return;
  const button = wrap.querySelector('.custom-select-button');
  const menu = wrap.querySelector('.custom-select-menu');
  const selected = select.options[select.selectedIndex];
  if (button) button.textContent = selected?.textContent || '';
  if (!menu) return;
  menu.innerHTML = Array.from(select.options).map(option => `
    <button type="button" class="custom-select-option ${option.value === select.value ? 'selected' : ''}" data-value="${escHtml(option.value)}">
      ${escHtml(option.textContent)}
    </button>`).join('');
  menu.querySelectorAll('.custom-select-option').forEach(optionBtn => {
    optionBtn.addEventListener('click', () => {
      select.value = optionBtn.dataset.value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
      wrap.classList.remove('open');
      syncCustomSelect(select);
    });
  });
}

async function pickDirectory() {
  if (!isTauri()) {
    return 'C:/MowisAI/mock-repositories';
  }
  const picked = await openDialogNative({ directory: true, multiple: false });
  return Array.isArray(picked) ? picked[0] : picked;
}

function setRepoStatus(message, type = 'info') {
  const status = $('repo-status');
  if (!status) return;
  status.textContent = message || '';
  status.className = `repo-status ${type}`;
}

function setRepoBusy(isBusy) {
  ['repo-pick-local', 'repo-pick-destination', 'repo-clone', 'repo-use'].forEach(id => {
    const el = $(id);
    if (el) el.disabled = !!isBusy;
  });
}

function showRepoModal() {
  const modal = $('repo-modal');
  if (!modal) return;
  modal.classList.remove('hidden');
  modal.setAttribute('aria-hidden', 'false');
  setRepoStatus('');
  if (!State.selectedRepo) {
    $('repo-selected')?.classList.add('hidden');
    const urlInput = $('repo-url');
    if (urlInput) urlInput.value = '';
    setText('repo-destination-label', 'No folder selected');
    State.cloneDestination = null;
  }
  $('repo-url')?.focus();
}

function hideRepoModal() {
  const modal = $('repo-modal');
  if (!modal) return;
  modal.classList.add('hidden');
  modal.setAttribute('aria-hidden', 'true');
}

function setRepoTab(tab) {
  document.querySelectorAll('.repo-tab').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.repoTab === tab);
  });
  document.querySelectorAll('.repo-panel').forEach(panel => {
    panel.classList.toggle('active', panel.id === `repo-panel-${tab}`);
  });
  setRepoStatus('');
}

function stageSelectedRepo(info) {
  State.selectedRepo = info;
  $('repo-selected')?.classList.remove('hidden');
  setText('repo-selected-name', info?.name || 'repository');
  setText('repo-selected-path', info?.path || '');
  renderRepoChip();
}

function renderRepoChip() {
  const btn = $('btn-repo-open');
  if (!btn) return;
  const repo = State.selectedRepo;
  const span = btn.querySelector('span');
  if (repo) {
    btn.classList.add('has-repo');
    btn.title = repo.name || 'repository';
    if (span) span.textContent = repo.name || 'repository';
  } else {
    btn.classList.remove('has-repo');
    btn.title = 'Add GitHub repository';
    if (span) span.textContent = 'Add GitHub repository';
  }
}

async function handlePickLocalRepo() {
  try {
    const path = await pickDirectory();
    if (!path) return;
    setRepoBusy(true);
    setRepoStatus('Checking repository...');
    const info = await invoke('validate_git_repository', { path });
    stageSelectedRepo(info);
    setRepoStatus('Repository ready', 'success');
  } catch (err) {
    setRepoStatus(String(err), 'error');
  } finally {
    setRepoBusy(false);
  }
}

async function handlePickCloneDestination() {
  try {
    const path = await pickDirectory();
    if (!path) return;
    State.cloneDestination = path;
    setText('repo-destination-label', path);
    setRepoStatus('');
  } catch (err) {
    setRepoStatus(String(err), 'error');
  }
}

async function handleCloneGitHubRepo() {
  const repoUrl = $('repo-url')?.value.trim();
  if (!parseGitHubRepoName(repoUrl)) {
    setRepoStatus('Paste a GitHub URL like https://github.com/owner/repo', 'error');
    return;
  }
  if (!State.cloneDestination) {
    setRepoStatus('Choose a destination folder', 'error');
    return;
  }

  try {
    setRepoBusy(true);
    setRepoStatus('Cloning repository...');
    const info = await invoke('clone_github_repo', {
      repoUrl,
      destinationParent: State.cloneDestination,
    });
    stageSelectedRepo(info);
    setRepoStatus('Repository cloned', 'success');
  } catch (err) {
    setRepoStatus(String(err), 'error');
  } finally {
    setRepoBusy(false);
  }
}

function useSelectedRepo() {
  if (!State.selectedRepo) {
    setRepoStatus('Select or clone a repository first', 'error');
    return;
  }
  renderRepoChip();
  hideRepoModal();
  toast('Repository attached', 'success');
}

function setupHandlers() {
  const taskClose = $('task-panel-toggle');
  if (taskClose) taskClose.textContent = 'x';

  // Nav
  document.querySelectorAll('.sb-item').forEach(item => {
    item.addEventListener('click', (e) => { e.preventDefault(); navigate(item.dataset.page); });
  });
  $('btn-sidebar-toggle')?.addEventListener('click', () => setSidebarCollapsed(!State.sidebarCollapsed));
  $('btn-chat-home')?.addEventListener('click', () => navigate('home'));
  $('btn-repo-open')?.addEventListener('click', showRepoModal);
  $('repo-close')?.addEventListener('click', hideRepoModal);
  $('repo-backdrop')?.addEventListener('click', hideRepoModal);
  $('repo-pick-local')?.addEventListener('click', handlePickLocalRepo);
  $('repo-pick-destination')?.addEventListener('click', handlePickCloneDestination);
  $('repo-clone')?.addEventListener('click', handleCloneGitHubRepo);
  $('repo-use')?.addEventListener('click', useSelectedRepo);
  $('repo-remove')?.addEventListener('click', () => {
    State.selectedRepo = null;
    State.cloneDestination = null;
    $('repo-selected')?.classList.add('hidden');
    const urlInput = $('repo-url');
    if (urlInput) urlInput.value = '';
    setText('repo-destination-label', 'No folder selected');
    setRepoStatus('');
    renderRepoChip();
  });
  document.querySelectorAll('.repo-tab').forEach(tab => {
    tab.addEventListener('click', () => setRepoTab(tab.dataset.repoTab));
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !$('repo-modal')?.classList.contains('hidden')) hideRepoModal();
  });

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

  // Show/hide zero-mode hint when home mode selector changes.
  $('home-mode')?.addEventListener('change', (e) => {
    updateHomeZeroHint(e.target.value);
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
    await invoke('stop_session');
    finalizeStreaming();
    setSessionActive(false, true);
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
  $('btn-engine-point')?.addEventListener('click', async () => {
    toast('Point to your QEMU installation', 'info');
    await handlePointInstallationFlow();
  });
  $('btn-engine-developer')?.addEventListener('click', () => {
    if (typeof showDeveloperBootstrap === 'function') showDeveloperBootstrap();
  });
  $('set-provider')?.addEventListener('change', (e) => {
    const rowGcp = $('row-gcp');
    const rowGcpRegion = $('row-gcp-region');
    const rowSaKey = $('row-sa-key');
    const showVertex = e.target.value === 'vertex';
    if (rowGcp) rowGcp.style.display = showVertex ? '' : 'none';
    if (rowGcpRegion) rowGcpRegion.style.display = showVertex ? '' : 'none';
    if (rowSaKey) rowSaKey.style.display = showVertex ? '' : 'none';
    if (e.tagName === 'SELECT') syncCustomSelect(e);
  });
  $('set-mode')?.addEventListener('change', (e) => {
    updateSettingsZeroHint(e.target.value);
  });

  // Sandbox toggle — update label live
  $('set-sandbox-enabled')?.addEventListener('change', (e) => {
    updateSandboxToggleLabel(e.target.checked);
  });

  // Discard sandbox button
  $('btn-discard-sandbox')?.addEventListener('click', async () => {
    try {
      await invoke('discard_sandbox');
      await refreshSandboxInfo();
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
        setVal('set-sa-key', path);
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
  if (!text) return;
  
  // Add user message to UI immediately
  appendChatMessage({ kind: 'user', content: text, ts: nowTs() });
  input.value = '';
  autoResize.call(input);
  
  // Send to backend
  try {
    await invoke('send_message', { message: text });
  } catch (err) {
    appendChatMessage({ kind: 'error', content: `Failed to send message: ${err}`, ts: nowTs() });
  }
}

function autoResize() {
  this.style.height = 'auto';
  this.style.height = Math.min(this.scrollHeight, 120) + 'px';
}

// ── Markdown-lite renderer ────────────────────────────────────────────────────

function mdLite(text) {
  return escHtml(text)
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\n/g, '<br>');
}

function escHtml(s) {
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

// ── Boot ──────────────────────────────────────────────────────────────────────

init().catch(console.error);


// ══════════════════════════════════════════════════════════════════════════════
// Developer Mode Bootstrap
// ══════════════════════════════════════════════════════════════════════════════

function showDeveloperBootstrap() {
  const modal = document.getElementById('developer-bootstrap-modal');
  if (!modal) return;
  modal.classList.remove('hidden');
  modal.setAttribute('aria-hidden', 'false');

  const statusEl = document.getElementById('dev-bs-status');
  if (statusEl) { statusEl.classList.add('hidden'); statusEl.textContent = ''; }

  // Load existing config to prefill fields
  invoke('get_developer_config').then(cfg => {
    setVal('dev-bs-qemu', cfg.qemu_path);
    setVal('dev-bs-iso', cfg.iso_path);
    setVal('dev-bs-disk', cfg.disk_path);
    setVal('dev-bs-port', cfg.agent_port || 8080);
  }).catch(() => {});
}

function hideDeveloperBootstrap() {
  const modal = document.getElementById('developer-bootstrap-modal');
  if (!modal) return;
  modal.classList.add('hidden');
  modal.setAttribute('aria-hidden', 'true');
}

async function startDeveloperBootstrap() {
  const qemu = document.getElementById('dev-bs-qemu')?.value?.trim();
  const iso  = document.getElementById('dev-bs-iso')?.value?.trim();
  const disk = document.getElementById('dev-bs-disk')?.value?.trim();
  const port = parseInt(document.getElementById('dev-bs-port')?.value || '8080', 10);

  if (!qemu || !iso || !disk) {
    toast('All three paths (QEMU, ISO, Disk) are required', 'error');
    return;
  }

  const statusEl = document.getElementById('dev-bs-status');
  if (statusEl) {
    statusEl.classList.remove('hidden');
    statusEl.textContent = 'Saving configuration...';
    statusEl.className = 'dev-bootstrap-status';
  }

  const config = {
    qemu_path: qemu,
    iso_path: iso,
    disk_path: disk,
    ram_mb: 512,
    agent_port: port,
    monitor_port: 4445,
    serial_port: 4444,
    mount_point: '/mnt/mowisai',
    disk_device: '/dev/sda',
    agentd_path: '/mnt/mowisai/agentd',
  };

  try {
    const result = await invoke('start_developer_bootstrap', { config });

    if (statusEl) {
      statusEl.textContent = 'Config saved. Restarting...';
      statusEl.className = 'dev-bootstrap-status ok';
    }

    toast('Configuration saved. Restarting to bootstrap QEMU...', 'success');

    // Reload the app — the bridge will detect the developer config and use
    // DeveloperLauncher to auto-boot the VM.
    setTimeout(() => window.location.reload(), 1500);
  } catch (e) {
    if (statusEl) {
      statusEl.textContent = String(e);
      statusEl.className = 'dev-bootstrap-status err';
    }
    toast('Bootstrap failed: ' + e, 'error');
  }
}

// ── Wire up bootstrap modal ──────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
  // Developer Mode button in engine setup modal
  const devBtn = document.getElementById('engine-developer');
  if (devBtn) {
    devBtn.addEventListener('click', () => {
      hideEngineSetupModal();
      showDeveloperBootstrap();
    });
  }

  // Cancel button
  const cancelBtn = document.getElementById('dev-bs-cancel');
  if (cancelBtn) {
    cancelBtn.addEventListener('click', hideDeveloperBootstrap);
  }

  // Start button
  const startBtn = document.getElementById('dev-bs-start');
  if (startBtn) {
    startBtn.addEventListener('click', startDeveloperBootstrap);
  }

  // Browse buttons
  const browseBtns = [
    { btnId: 'dev-bs-browse-qemu', inputId: 'dev-bs-qemu', title: 'Select QEMU Binary' },
    { btnId: 'dev-bs-browse-iso',  inputId: 'dev-bs-iso',  title: 'Select ISO File' },
    { btnId: 'dev-bs-browse-disk', inputId: 'dev-bs-disk', title: 'Select Disk Image' },
  ];
  browseBtns.forEach(({ btnId, inputId, title }) => {
    const btn = document.getElementById(btnId);
    if (btn) {
      btn.addEventListener('click', async () => {
        if (!isTauri()) {
          toast('File browser not available in web mode', 'error');
          return;
        }
        try {
          const selected = await openDialogNative({ title, multiple: false, directory: false });
          if (selected) {
            const input = document.getElementById(inputId);
            if (input) input.value = Array.isArray(selected) ? selected[0] : selected;
          }
        } catch (e) {
          console.error('File dialog error:', e);
        }
      });
    }
  });

  // Close on backdrop click
  const bootstrapModal = document.getElementById('developer-bootstrap-modal');
  if (bootstrapModal) {
    const backdrop = bootstrapModal.querySelector('.engine-modal-backdrop');
    if (backdrop) {
      backdrop.addEventListener('click', hideDeveloperBootstrap);
    }
  }
});
