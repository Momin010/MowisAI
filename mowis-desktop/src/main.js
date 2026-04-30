/**
 * MowisAI Desktop — Minimal Chat-First Application
 * Tauri v2 + Vite. Graceful browser fallback.
 */

// ── Tauri bridge ──────────────────────────────────────────────────────────────

let _invoke = null;
let _listen = null;
let _openDialog = null;

async function loadTauri() {
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen }  = await import('@tauri-apps/api/event');
    const { open } = await import('@tauri-apps/plugin-dialog');
    _invoke = invoke;
    _listen = listen;
    _openDialog = open;
    return true;
  } catch { return false; }
}

async function invoke(cmd, args = {}) {
  if (_invoke) return _invoke(cmd, args);
  return mockInvoke(cmd, args);
}

async function listen(event, cb) {
  if (_listen) return _listen(event, cb);
  // Browser: events are dispatched via dispatchEvent below
  const handler = (e) => cb(e.detail);
  window.addEventListener(`tauri:${event}`, handler);
  return () => window.removeEventListener(`tauri:${event}`, handler);
}

function dispatchMockEvent(event, payload) {
  window.dispatchEvent(new CustomEvent(`tauri:${event}`, { detail: { payload } }));
}

// ── Mock backend (browser dev) ────────────────────────────────────────────────

const MockState = {
  config: { socket_path: '/tmp/agentd.sock', max_agents: 100, mode: 'auto', provider: 'gemini', model: 'gemini-2.0-flash', api_key: '', gcp_project: '' },
  messages: [],
  tasks: {},
  current_session_id: null,
  sessions: {},
  session_history: [],
  usage_history: [],
  daemon: false,
  tokens: 0,
  tool_calls: 0,
};

const MOCK_STORE_KEY = 'mowisai_mock_state_v2';

function loadMockState() {
  try {
    const raw = localStorage.getItem(MOCK_STORE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    Object.assign(MockState, parsed);
  } catch {}
}

function saveMockState() {
  try {
    localStorage.setItem(MOCK_STORE_KEY, JSON.stringify(MockState));
  } catch {}
}

loadMockState();

function mockInvoke(cmd, args) {
  switch (cmd) {
    case 'get_config':          return Promise.resolve(MockState.config);
    case 'save_config':         MockState.config = args.config; saveMockState(); return Promise.resolve();
    case 'get_messages':        return Promise.resolve(MockState.messages);
    case 'get_tasks':           return Promise.resolve(Object.values(MockState.tasks));
    case 'get_session_history': return Promise.resolve(MockState.session_history);
    case 'get_usage_history':   return Promise.resolve(MockState.usage_history);
    case 'get_current_session': return Promise.resolve(mockCurrentSession());
    case 'load_session':        return Promise.resolve(mockLoadSession(args.sessionId || args.session_id));
    case 'clear_current_session': MockState.current_session_id = null; MockState.messages = []; MockState.tasks = {}; MockState.tokens = 0; MockState.tool_calls = 0; saveMockState(); return Promise.resolve();
    case 'window_control':      return Promise.resolve();
    case 'get_daemon_status':   return Promise.resolve(MockState.daemon);
    case 'check_daemon':        return Promise.resolve(false);
    case 'get_system_info':     return Promise.resolve({ os: 'web', arch: 'x64', version: '0.1.0' });
    case 'get_stats':           return Promise.resolve(mockStats());
    case 'validate_git_repository': return Promise.resolve(mockGitInfo(args.path, 'local', null));
    case 'clone_github_repo':    return Promise.resolve(mockCloneGitHubRepo(args.repoUrl || args.repo_url, args.destinationParent || args.destination_parent));
    case 'start_session':       return mockStartSession(args);
    case 'stop_session':        return mockStopSession();
    default: return Promise.reject(`Unknown command: ${cmd}`);
  }
}

function mockGitInfo(path, source = 'local', repoUrl = null) {
  const clean = String(path || '').replace(/[\\\/]+$/, '');
  const name = clean.split(/[\\\/]/).pop() || 'repository';
  return {
    path: clean || `/mock/${name}`,
    name,
    branch: 'main',
    remote_url: repoUrl,
    source,
    repo_url: repoUrl,
  };
}

async function mockCloneGitHubRepo(repoUrl, destinationParent) {
  const parsed = parseGitHubRepoName(repoUrl);
  if (!parsed) throw new Error('Use a GitHub URL like https://github.com/owner/repo');
  const base = String(destinationParent || '/mock/repos').replace(/[\\\/]+$/, '');
  await delay(600);
  return mockGitInfo(`${base}/${parsed.repo}`, 'github', repoUrl);
}

function mockStats() {
  const usageTokens = MockState.usage_history.reduce((sum, item) => sum + (item.tokens || 0), 0);
  const usageTools = MockState.usage_history.reduce((sum, item) => sum + (item.tool_calls || 0), 0);
  const usageTasks = MockState.usage_history.reduce((sum, item) => sum + (item.task_count || 0), 0);
  const current = MockState.current_session_id ? MockState.sessions[MockState.current_session_id] : null;
  const active = current?.summary?.status === 'running';
  return {
    tasks_total: Object.keys(MockState.tasks).length,
    tasks_running: Object.values(MockState.tasks).filter(t => t.status === 'running').length,
    tasks_done: Object.values(MockState.tasks).filter(t => t.status === 'complete').length,
    tasks_failed: Object.values(MockState.tasks).filter(t => t.status === 'failed').length,
    tokens_total: MockState.tokens,
    tool_calls: MockState.tool_calls,
    lifetime_tokens: usageTokens + (active ? MockState.tokens : 0),
    lifetime_tool_calls: usageTools + (active ? MockState.tool_calls : 0),
    lifetime_tasks: usageTasks + (active ? Object.values(MockState.tasks).filter(t => t.status === 'complete').length : 0),
    daemon_connected: false,
  };
}

function mockCurrentSession() {
  const id = MockState.current_session_id;
  if (!id || !MockState.sessions[id]) return null;
  return MockState.sessions[id];
}

function mockLoadSession(id) {
  const detail = MockState.sessions[id];
  if (!detail) throw new Error(`session not found: ${id}`);
  MockState.current_session_id = id;
  MockState.messages = detail.messages || [];
  MockState.tasks = Object.fromEntries((detail.tasks || []).map(t => [t.id, t]));
  MockState.tokens = detail.tokens_total || 0;
  MockState.tool_calls = detail.tool_calls_total || 0;
  saveMockState();
  return detail;
}

function mockSyncSession(status = 'running') {
  const id = MockState.current_session_id;
  if (!id) return;
  const tasks = Object.values(MockState.tasks);
  const existing = MockState.sessions[id];
  const summary = existing?.summary || {
    id,
    prompt: MockState.messages.find(m => m.kind === 'user')?.content?.slice(0, 80) || '',
    status,
    started_at: nowTs(),
    completed_at: null,
    task_count: 0,
    tasks_done: 0,
  };
  summary.status = status;
  summary.task_count = tasks.length;
  summary.tasks_done = tasks.filter(t => t.status === 'complete').length;
  if (status !== 'running') summary.completed_at = nowTs();
  MockState.sessions[id] = {
    summary,
    messages: MockState.messages,
    tasks,
    tokens_total: MockState.tokens,
    tool_calls_total: MockState.tool_calls,
  };
  const idx = MockState.session_history.findIndex(s => s.id === id);
  if (idx >= 0) MockState.session_history[idx] = summary;
  else MockState.session_history.push(summary);
  saveMockState();
}

function mockStopSession() {
  MockState.messages.push({ kind: 'system', content: 'Session stopped.', ts: nowTs() });
  mockSyncSession('stopped');
  return Promise.resolve();
}

async function mockStartSession({ prompt, mode }) {
  const id = `sess-${Date.now().toString(36)}`;
  MockState.current_session_id = id;
  MockState.messages = [{ kind: 'user', content: prompt, ts: nowTs() }];
  MockState.tasks = {};
  MockState.tokens = 0;
  MockState.tool_calls = 0;
  MockState.sessions[id] = {
    summary: { id, prompt: prompt.slice(0, 80), status: 'running', started_at: nowTs(), completed_at: null, task_count: 0, tasks_done: 0 },
    messages: MockState.messages,
    tasks: [],
    tokens_total: 0,
    tool_calls_total: 0,
  };
  MockState.session_history.push(MockState.sessions[id].summary);
  saveMockState();

  // Run async simulation
  runBrowserSimulation(id, prompt, mode || MockState.config.mode);
  return id;
}

async function runBrowserSimulation(sessionId, prompt, mode) {
  await delay(300);

  // Plan card
  const plan = { kind: 'plan', sandboxes: ['frontend', 'backend', 'verification'], task_count: 20, agent_count: 24, mode: mode || 'auto', ts: nowTs() };
  MockState.messages.push(plan);
  mockSyncSession('running');
  dispatchMockEvent('chat_message', plan);

  await delay(400);

  // Streaming agent response
  const chunks = [
    `Understood. Launching 7-layer orchestration pipeline…\n\n`,
    `**Task:** *${prompt.slice(0,70)}*\n\n`,
    `Sandboxes provisioned with overlayfs 3-level CoW. Scheduler active.\n`,
    `Agents dispatched across frontend, backend, and verification sandboxes.\n`,
  ];

  let streaming = true;
  for (const chunk of chunks) {
    dispatchMockEvent('agent_chunk', { chunk });
    await delay(100);
  }

  // Emit tasks
  const sampleTasks = [
    ['Implement OAuth2 middleware', 'backend'],
    ['Build REST API endpoints', 'backend'],
    ['Create React components', 'frontend'],
    ['Set up database schema', 'backend'],
    ['Implement WebSocket streaming', 'backend'],
    ['Write unit tests', 'verification'],
    ['Configure CI/CD pipeline', 'backend'],
    ['Optimise query performance', 'backend'],
    ['Implement rate limiting', 'backend'],
    ['Build file upload service', 'backend'],
    ['Generate API documentation', 'backend'],
    ['Set up error monitoring', 'verification'],
    ['Implement caching layer', 'backend'],
    ['Build admin panel', 'frontend'],
    ['Configure SSL/TLS', 'backend'],
    ['Write integration tests', 'verification'],
    ['Implement search', 'backend'],
    ['Build notification system', 'frontend'],
    ['Set up logging', 'verification'],
    ['Create deployment scripts', 'backend'],
  ];

  const taskIds = [];
  for (let i = 0; i < sampleTasks.length; i++) {
    const id = `t${String(i).padStart(4,'0')}`;
    const [desc, sb] = sampleTasks[i];
    const task = {
      id,
      description: desc,
      sandbox: sb,
      status: 'pending',
      started_at: null,
      completed_at: null,
      files: mockTaskFiles(sb, i),
      summary: `Implemented ${desc} in the ${sb} sandbox.`,
      views: mockTaskViews(sb),
    };
    MockState.tasks[id] = task;
    mockSyncSession('running');
    dispatchMockEvent('task_added', task);
    taskIds.push(id);
    await delay(25);
  }

  // Run tasks in waves
  const waveSize = 4;
  for (let i = 0; i < taskIds.length; i += waveSize) {
    const batch = taskIds.slice(i, i + waveSize);
    for (const id of batch) {
      MockState.tasks[id].status = 'running';
      MockState.tasks[id].started_at = nowTs();
      dispatchMockEvent('task_updated', { id, status: 'running' });
    }
    const tokDelta = batch.length * (60 + i * 5);
    MockState.tokens += tokDelta;
    MockState.tool_calls += batch.length;
    mockSyncSession('running');
    dispatchMockEvent('stats_tick', { tasks_done: i, active_agents: batch.length, tokens_total: MockState.tokens });
    await delay(200);
    for (const id of batch) {
      MockState.tasks[id].status = 'complete';
      MockState.tasks[id].completed_at = nowTs();
      dispatchMockEvent('task_updated', { id, status: 'complete' });
    }
    mockSyncSession('running');
  }

  // Final agent message
  const finalChunks = [
    `\n**Layer 5 (Merge):** parallel tree-pattern merge — 2 conflicts resolved\n`,
    `**Layer 6 (Verification):** all tests passed in 2 rounds\n`,
    `**Layer 7 (Output):** cross-sandbox integration merge complete ✓\n`,
  ];
  for (const chunk of finalChunks) {
    dispatchMockEvent('agent_chunk', { chunk });
    await delay(150);
  }

  await delay(200);
  mockSyncSession('done');
  MockState.usage_history.push({
    session_id: sessionId,
    prompt_short: prompt.slice(0, 80),
    ts: nowTs(),
    task_count: sampleTasks.length,
    tokens: MockState.tokens,
    tool_calls: MockState.tool_calls,
    duration_secs: 30,
    status: 'done',
  });
  saveMockState();
  dispatchMockEvent('session_complete', {});
}

function mockTaskFiles(sandbox, index) {
  if (sandbox === 'frontend') return [`src/views/session-${index}.tsx`, `src/styles/tasks-${index}.css`];
  if (sandbox === 'backend') return [`src/services/task-${index}.rs`, `src/api/session-${index}.rs`];
  return [`tests/session-${index}.rs`, `tests/fixtures/task-${index}.json`];
}

function mockTaskViews(sandbox) {
  if (sandbox === 'frontend') return ['Session timeline', 'Task inspector'];
  if (sandbox === 'backend') return ['API contract', 'Execution trace'];
  return ['Verification report'];
}

function delay(ms) { return new Promise(r => setTimeout(r, ms)); }
function nowTs() { return Math.floor(Date.now() / 1000); }

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
  stats: { tasks_total: 0, tasks_done: 0, tasks_running: 0, tokens_total: 0, tool_calls: 0 },
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

function fmtNumber(n) {
  if (!n) return '0';
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'k';
  return String(n);
}

function fmtTs(ts) {
  if (!ts) return '—';
  const d = new Date(ts * 1000);
  return d.toLocaleDateString() + ' ' + d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
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
  try {
    await invoke('window_control', { action });
  } catch {}

  if (!_invoke) return;
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

  if (fill) fill.style.width = '5%';
  if (hint) hint.textContent = 'Starting…';

  // Forward real setup-progress events from BackendBridge while booting.
  let unlisten = null;
  let resolved = false;
  const backendReady = new Promise(resolve => {
    listen('setup_progress', (e) => {
      const p = e.payload;
      if (!p) return;
      if (fill) fill.style.width = Math.max(5, p.pct) + '%';
      if (hint) hint.textContent = p.message;
      if ((p.stage === 'ready' || p.stage === 'error') && !resolved) {
        resolved = true;
        resolve(p);
      }
    }).then(u => { unlisten = u; });
  });

  // Show the splash for at least 1 s; give the backend up to 4 s before
  // showing the app regardless (it keeps starting in the background).
  await Promise.race([backendReady, delay(4000)]);
  if (unlisten) unlisten();

  if (fill) fill.style.width = '100%';
  await delay(200);
  $('splash')?.classList.add('hidden');
  $('app')?.classList.remove('hidden');
}

// ── Init ──────────────────────────────────────────────────────────────────────

async function init() {
  await loadTauri();
  setupWindowControls();   // non-blocking, sets up dot handlers
  await runSplash();

  // Load config
  try { State.config = await invoke('get_config'); } catch { State.config = { socket_path: '/tmp/agentd.sock', max_agents: 100, mode: 'auto', provider: 'gemini', model: 'gemini-2.0-flash', api_key: '', gcp_project: '' }; }

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
  const banner = document.createElement('div');
  banner.id = 'offline-banner';
  banner.className = 'offline-banner';
  banner.innerHTML = `
    <div class="offline-banner-title">⚙ Engine not connected</div>
    <div class="offline-banner-body">${guidance.replace(/\n/g, '<br>')}</div>
    <div class="offline-banner-footer">
      The app is fully usable — sessions will run in simulation mode until the engine is available.
    </div>`;
  const homeEmpty = $('home-empty');
  if (homeEmpty) homeEmpty.prepend(banner);
}

function removeOfflineBanner() {
  $('offline-banner')?.remove();
}

function setDaemonStatus(on) {
  State.daemonConnected = on;
  const dot = $('daemon-dot');
  const sbDot = $('sb-daemon-dot');
  const sbLabel = $('sb-daemon-label');
  const statusEl = $('sb-daemon-status');

  if (dot)     dot.classList.toggle('on', on);
  if (sbDot)   sbDot.classList.toggle('on', on);
  if (sbLabel) sbLabel.textContent = on ? 'daemon online' : 'daemon offline';
  setText('sb-daemon-status', `daemon: ${on ? 'online' : 'offline'}`);
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

    // Re-enable send
    setSessionActive(false, /* keep chat */ true);

    updateStatusBar();
    toast('Session complete', 'success');
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
    row = createPlanCard(msg);
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
      ${(msg.sandboxes || []).map(s => `<span class="plan-sb">${s}</span>`).join('')}
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
  if (openBtn) openBtn.style.display = hasTasks ? '' : 'none';
  if (!hasTasks) State.taskPanelOpen = false;
  if (panel) panel.style.display = hasTasks && State.taskPanelOpen ? '' : 'none';
  if (chat) chat.classList.toggle('task-panel-open', hasTasks && State.taskPanelOpen);
}

function renderTaskPanel() {
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
  State.taskPanelOpen = open && Object.keys(State.tasks).length > 0;
  updateTaskPanelVisibility();
  if (State.taskPanelOpen) renderTaskPanel();
}

// ── Status bar ────────────────────────────────────────────────────────────────

function updateStatusBar() {
  const tasks = Object.values(State.tasks);
  const done = tasks.filter(t => t.status === 'complete').length;
  const total = tasks.length;

  setText('sb-session-info', State.sessionId ? `session ${State.sessionId.slice(0,10)}` : 'No session');
  setText('sb-tasks', `${done} / ${total} tasks`);
}

// ── Sessions page ─────────────────────────────────────────────────────────────

async function renderSessionsPage() {
  try {
    const hist = await invoke('get_session_history');
    const list = $('sessions-list');
    const empty = $('sessions-empty');

    if (!hist.length) {
      if (empty) empty.style.display = '';
      if (list) list.style.display = 'none';
      return;
    }
    if (empty) empty.style.display = 'none';
    if (list) list.style.display = '';

    setText('sb-badge-sessions', String(hist.length));

    list.innerHTML = [...hist].reverse().map(s => `
      <div class="session-card" data-id="${s.id}">
        <div class="sc-prompt">${escHtml(s.prompt || '—')}</div>
        <div class="sc-meta">
          <span class="sc-status ${s.status}">${s.status.toUpperCase()}</span>
          <span>${s.tasks_done}/${s.task_count} tasks</span>
          <span>${fmtTs(s.started_at)}</span>
        </div>
      </div>`).join('');

    list.querySelectorAll('.session-card').forEach(card => {
      card.addEventListener('click', () => {
        openSession(card.dataset.id);
      });
    });
  } catch (e) {
    console.error(e);
  }
}

// ── Usage page ────────────────────────────────────────────────────────────────

async function renderUsagePage() {
  try {
    const [stats, hist, usage] = await Promise.all([invoke('get_stats'), invoke('get_session_history'), invoke('get_usage_history')]);

    setText('us-sessions', hist.length);
    setText('us-tasks', fmtNumber(stats.lifetime_tasks ?? (stats.tasks_done + stats.tasks_total)));
    setText('us-tokens', fmtNumber(stats.lifetime_tokens ?? stats.tokens_total));
    setText('us-tools', fmtNumber(stats.lifetime_tool_calls ?? stats.tool_calls));
    renderUsageChart(usage, stats);

    const wrap = $('usage-sessions-table');
    if (!wrap) return;

    if (!hist.length) {
      wrap.innerHTML = '<div class="empty-state small"><div class="empty-text">No history yet</div></div>';
      return;
    }

    wrap.innerHTML = `<table class="usage-table">
      <thead><tr>
        <th>Prompt</th><th>Status</th><th>Tasks</th><th>Started</th>
      </tr></thead>
      <tbody>
        ${[...hist].reverse().map(s => `<tr>
          <td class="tx">${escHtml(s.prompt.slice(0,60))}${s.prompt.length>60?'…':''}</td>
          <td><span class="sc-status ${s.status}">${s.status}</span></td>
          <td>${s.tasks_done}/${s.task_count}</td>
          <td>${fmtTs(s.started_at)}</td>
        </tr>`).join('')}
      </tbody>
    </table>`;
  } catch {}
}

// ── Settings ──────────────────────────────────────────────────────────────────

function renderUsageChart(usage, stats) {
  const el = $('usage-chart');
  if (!el) return;
  const rows = [...(usage || [])].slice(-7);
  const totalTokens = stats?.lifetime_tokens ?? rows.reduce((sum, item) => sum + (item.tokens || 0), 0);
  setText('usage-total-label', `${fmtNumber(totalTokens)} tokens`);
  setText('usage-chart-meta', rows.length ? `Last ${rows.length} sessions` : 'No completed sessions');

  if (!rows.length) {
    el.innerHTML = `<svg viewBox="0 0 640 170" role="img" aria-label="No usage history">
      <line class="axis" x1="36" y1="130" x2="604" y2="130"></line>
      <text class="trend-label" x="250" y="86">No usage recorded yet</text>
    </svg>`;
    return;
  }

  const width = 640;
  const height = 170;
  const pad = 34;
  const max = Math.max(...rows.map(item => item.tokens || 0), 1);
  const step = rows.length === 1 ? 0 : (width - pad * 2) / (rows.length - 1);
  const points = rows.map((item, i) => {
    const x = rows.length === 1 ? width / 2 : pad + i * step;
    const y = height - pad - ((item.tokens || 0) / max) * (height - pad * 2);
    return { x, y, item };
  });
  const line = points.map(p => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' ');
  const area = `${pad},${height - pad} ${line} ${points[points.length - 1].x.toFixed(1)},${height - pad}`;

  el.innerHTML = `<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="Usage trend">
    <line class="axis" x1="${pad}" y1="${height - pad}" x2="${width - pad}" y2="${height - pad}"></line>
    <polyline class="trend-fill" points="${area}"></polyline>
    <polyline class="trend-line" points="${line}"></polyline>
    ${points.map((p, i) => `
      <circle class="trend-dot" cx="${p.x.toFixed(1)}" cy="${p.y.toFixed(1)}" r="4"></circle>
      <text class="trend-label" x="${(p.x - 16).toFixed(1)}" y="${height - 10}">${i + 1}</text>
    `).join('')}
  </svg>`;
}

function loadSettings() {
  const c = State.config || {};
  setVal('set-provider', c.provider || 'gemini');
  setVal('set-model', c.model || '');
  setVal('set-gcp', c.gcp_project || '');
  setVal('set-socket', c.socket_path || '/tmp/agentd.sock');
  setVal('set-mode', c.mode || 'auto');
  setVal('set-max-agents', c.max_agents || 100);
  const rowGcp = $('row-gcp');
  if (rowGcp) rowGcp.style.display = (c.provider === 'gemini') ? '' : 'none';
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

function parseGitHubRepoName(repoUrl) {
  const raw = String(repoUrl || '').trim();
  const path = raw.startsWith('https://github.com/')
    ? raw.slice('https://github.com/'.length)
    : raw.startsWith('git@github.com:')
      ? raw.slice('git@github.com:'.length)
      : '';
  if (!path) return null;
  const clean = path.split(/[?#]/)[0].replace(/^\/+|\/+$/g, '').replace(/\.git$/, '');
  const parts = clean.split('/');
  if (parts.length !== 2 || !parts[0] || !parts[1]) return null;
  return { owner: parts[0], repo: parts[1] };
}

async function pickDirectory() {
  if (!_openDialog) {
    return 'C:/MowisAI/mock-repositories';
  }
  const picked = await _openDialog({ directory: true, multiple: false });
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

  // Settings
  $('btn-save-settings')?.addEventListener('click', saveSettings);
  $('set-provider')?.addEventListener('change', (e) => {
    const rowGcp = $('row-gcp');
    if (rowGcp) rowGcp.style.display = e.target.value === 'gemini' ? '' : 'none';
  });
  $('btn-test-socket')?.addEventListener('click', async () => {
    const statusEl = $('socket-status');
    try {
      const on = await invoke('check_daemon');
      if (statusEl) { statusEl.textContent = on ? '✓ Connected' : '✗ Not reachable'; statusEl.className = 'socket-status ' + (on ? 'ok' : 'err'); }
      toast(on ? 'Daemon connected' : 'Daemon not reachable', on ? 'success' : 'error');
    } catch { if (statusEl) { statusEl.textContent = '✗ Error'; statusEl.className = 'socket-status err'; } }
  });

  // Auto-resize compose textarea
  $('chat-input')?.addEventListener('input', autoResize);
}

function sendChatMessage() {
  const input = $('chat-input');
  if (!input) return;
  const text = input.value.trim();
  if (!text) return;
  appendChatMessage({ kind: 'user', content: text, ts: nowTs() });
  input.value = '';
  autoResize.call(input);
  // TODO: wire to follow-up command once backend supports it
  toast('Follow-up sent (requires running daemon)', 'info');
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
