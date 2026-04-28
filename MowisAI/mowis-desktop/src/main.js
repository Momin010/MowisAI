/**
 * MowisAI Desktop — Minimal Chat-First Application
 * Tauri v2 + Vite. Graceful browser fallback.
 */

// ── Tauri bridge ──────────────────────────────────────────────────────────────

let _invoke = null;
let _listen = null;

async function loadTauri() {
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen }  = await import('@tauri-apps/api/event');
    _invoke = invoke;
    _listen = listen;
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
  session_history: [],
  usage_history: [],
  daemon: false,
  tokens: 0,
  tool_calls: 0,
};

function mockInvoke(cmd, args) {
  switch (cmd) {
    case 'get_config':          return Promise.resolve(MockState.config);
    case 'save_config':         MockState.config = args.config; return Promise.resolve();
    case 'get_messages':        return Promise.resolve(MockState.messages);
    case 'get_tasks':           return Promise.resolve(Object.values(MockState.tasks));
    case 'get_session_history': return Promise.resolve(MockState.session_history);
    case 'get_usage_history':   return Promise.resolve(MockState.usage_history);
    case 'get_daemon_status':   return Promise.resolve(MockState.daemon);
    case 'check_daemon':        return Promise.resolve(false);
    case 'get_system_info':     return Promise.resolve({ os: 'web', arch: 'x64', version: '0.1.0' });
    case 'get_stats':           return Promise.resolve({ tasks_total: Object.keys(MockState.tasks).length, tasks_running: 0, tasks_done: Object.values(MockState.tasks).filter(t=>t.status==='complete').length, tasks_failed: 0, tokens_total: MockState.tokens, tool_calls: MockState.tool_calls, daemon_connected: false });
    case 'start_session':       return mockStartSession(args);
    case 'stop_session':        return Promise.resolve();
    default: return Promise.reject(`Unknown command: ${cmd}`);
  }
}

async function mockStartSession({ prompt, mode }) {
  const id = `sess-${Date.now().toString(36)}`;
  MockState.messages = [{ kind: 'user', content: prompt, ts: nowTs() }];
  MockState.tasks = {};
  MockState.tokens = 0;
  MockState.tool_calls = 0;

  // Run async simulation
  runBrowserSimulation(id, prompt, mode || MockState.config.mode);
  return id;
}

async function runBrowserSimulation(sessionId, prompt, mode) {
  await delay(300);

  // Plan card
  const plan = { kind: 'plan', sandboxes: ['frontend', 'backend', 'verification'], task_count: 20, agent_count: 24, mode: mode || 'auto', ts: nowTs() };
  MockState.messages.push(plan);
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
    const task = { id, description: desc, sandbox: sb, status: 'pending', started_at: null, completed_at: null };
    MockState.tasks[id] = task;
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
      dispatchMockEvent('task_updated', { id, status: 'running' });
    }
    const tokDelta = batch.length * (60 + i * 5);
    MockState.tokens += tokDelta;
    MockState.tool_calls += batch.length;
    dispatchMockEvent('stats_tick', { tasks_done: i, active_agents: batch.length, tokens_total: MockState.tokens });
    await delay(200);
    for (const id of batch) {
      MockState.tasks[id].status = 'complete';
      dispatchMockEvent('task_updated', { id, status: 'complete' });
    }
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
  dispatchMockEvent('session_complete', {});
  MockState.session_history.push({
    id: sessionId,
    prompt: prompt.slice(0, 80),
    status: 'done',
    started_at: nowTs() - 30,
    completed_at: nowTs(),
    task_count: sampleTasks.length,
    tasks_done: sampleTasks.length,
  });
}

function delay(ms) { return new Promise(r => setTimeout(r, ms)); }
function nowTs() { return Math.floor(Date.now() / 1000); }

// ── App State ─────────────────────────────────────────────────────────────────

const State = {
  page: 'home',
  sessionActive: false,
  sessionId: null,
  taskPanelOpen: true,
  tasks: {},
  streamingContent: '',
  isStreaming: false,
  daemonConnected: false,
  config: null,
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

function navigate(page) {
  State.page = page;
  document.querySelectorAll('.sb-item').forEach(i => i.classList.toggle('active', i.dataset.page === page));
  document.querySelectorAll('.page').forEach(p => p.classList.toggle('active', p.id === `page-${page}`));

  const names = { home: 'Home', sessions: 'Sessions', usage: 'Usage', settings: 'Settings' };
  setText('tl-page', names[page] || page);

  if (page === 'sessions') renderSessionsPage();
  if (page === 'usage') renderUsagePage();
  if (page === 'settings') loadSettings();
}

// ── Splash ────────────────────────────────────────────────────────────────────

async function runSplash() {
  const fill = $('splash-fill');
  const hint = $('splash-hint');
  const steps = [[20,'Loading…'],[45,'Checking daemon…'],[70,'Loading config…'],[100,'Ready']];
  for (const [pct, msg] of steps) {
    if (fill) fill.style.width = pct + '%';
    if (hint) hint.textContent = msg;
    await delay(180);
  }
  await delay(150);
  $('splash')?.classList.add('hidden');
  $('app')?.classList.remove('hidden');
}

// ── Init ──────────────────────────────────────────────────────────────────────

async function init() {
  await loadTauri();
  await runSplash();

  // Load config
  try { State.config = await invoke('get_config'); } catch { State.config = { socket_path: '/tmp/agentd.sock', max_agents: 100, mode: 'auto', provider: 'gemini', model: 'gemini-2.0-flash', api_key: '', gcp_project: '' }; }

  // System info
  try {
    const info = await invoke('get_system_info');
    setText('about-meta', `${info.os} · ${info.arch} · MowisAI v${info.version}`);
    setText('tl-version', `v${info.version}`);
  } catch {}

  // Check daemon
  checkDaemon();

  // Setup event listeners
  await setupListeners();

  // Setup UI handlers
  setupHandlers();

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

// ── Daemon check ──────────────────────────────────────────────────────────────

async function checkDaemon() {
  try {
    const on = await invoke('check_daemon');
    setDaemonStatus(on);
  } catch { setDaemonStatus(false); }
  setTimeout(checkDaemon, 8000);
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
    State.tasks[task.id] = task;
    renderTaskPanel();
    updateTaskPanelVisibility();
  });

  await listen('task_updated', (e) => {
    const { id, status } = e.payload || {};
    if (id && State.tasks[id]) {
      State.tasks[id].status = status;
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
    toast('Session complete ✓', 'success');
  });
}

// ── Session start ─────────────────────────────────────────────────────────────

async function startSession(prompt, mode) {
  if (!prompt.trim()) { toast('Enter a task description', 'error'); return; }

  // Reset chat
  State.tasks = {};
  State.isStreaming = false;
  State.streamingContent = '';

  const chatMessages = $('chat-messages');
  if (chatMessages) chatMessages.innerHTML = '';

  const taskPanelBody = $('task-panel-body');
  if (taskPanelBody) taskPanelBody.innerHTML = '';

  // Show chat, hide empty
  $('home-empty')?.classList.add('hidden');
  $('home-chat')?.classList.remove('hidden');

  setSessionActive(true);

  // Show user message immediately
  appendChatMessage({ kind: 'user', content: prompt, ts: nowTs() });

  // Clear task panel
  updateTaskPanelVisibility();

  try {
    const id = await invoke('start_session', { prompt, mode: mode || 'auto' });
    State.sessionId = id;
    setText('compose-session-info', `session ${id.slice(0,12)}`);
  } catch (err) {
    appendChatMessage({ kind: 'error', content: String(err), ts: nowTs() });
    setSessionActive(false);
  }

  // Navigate to home
  navigate('home');
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
  const hasTasks = Object.keys(State.tasks).length > 0;
  if (panel) panel.style.display = hasTasks ? '' : 'none';
}

function renderTaskPanel() {
  const body = $('task-panel-body');
  const counts = $('task-counts');
  const fill = $('task-progress-fill');
  if (!body) return;

  const tasks = Object.values(State.tasks);
  const done = tasks.filter(t => t.status === 'complete').length;
  const total = tasks.length;

  if (counts) counts.textContent = `${done} / ${total}`;
  if (fill) fill.style.width = total > 0 ? `${(done / total * 100).toFixed(1)}%` : '0%';

  body.innerHTML = tasks.map(t => `
    <div class="task-row">
      <span class="task-dot ${t.status}"></span>
      <span class="task-desc">${escHtml(t.description)}</span>
      <span class="task-sb">${t.sandbox || ''}</span>
    </div>`).join('');

  updateStatusBar();
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
        toast(`Session ${card.dataset.id.slice(0,8)}`);
      });
    });
  } catch (e) {
    console.error(e);
  }
}

// ── Usage page ────────────────────────────────────────────────────────────────

async function renderUsagePage() {
  try {
    const [stats, hist] = await Promise.all([invoke('get_stats'), invoke('get_session_history')]);

    setText('us-sessions', hist.length);
    setText('us-tasks', fmtNumber(stats.tasks_done + stats.tasks_total));
    setText('us-tokens', fmtNumber(stats.tokens_total));
    setText('us-tools', fmtNumber(stats.tool_calls));

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

function setVal(id, val) { const e = $(id); if (!e) return; if (e.type === 'checkbox') e.checked = !!val; else e.value = val ?? ''; }
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

function setupHandlers() {
  // Nav
  document.querySelectorAll('.sb-item').forEach(item => {
    item.addEventListener('click', (e) => { e.preventDefault(); navigate(item.dataset.page); });
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
    const sys = { kind: 'system', content: '— session stopped —', ts: nowTs() };
    appendChatMessage(sys);
    toast('Session stopped');
  });

  // Task panel toggle
  $('task-panel-toggle')?.addEventListener('click', () => {
    State.taskPanelOpen = !State.taskPanelOpen;
    const body = $('task-panel-body');
    const btn = $('task-panel-toggle');
    if (body) body.classList.toggle('collapsed', !State.taskPanelOpen);
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
