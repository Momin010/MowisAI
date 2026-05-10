/**
 * MowisAI Desktop — Mock backend (browser dev / simulation)
 */

import { delay, nowTs, parseGitHubRepoName } from './utils.js';

// Inline dispatchMockEvent to avoid circular dependency with bridge.js
// (bridge.js imports mock.js, so mock.js cannot import bridge.js)
function dispatchMockEvent(event, payload) {
  window.dispatchEvent(new CustomEvent(`tauri:${event}`, { detail: { payload } }));
}

export { dispatchMockEvent };

// ── Mock State ────────────────────────────────────────────────────────────────

export const MockState = {
  config: { socket_path: '/tmp/agentd.sock', max_agents: 100, mode: 'auto', provider: 'gemini', model: 'gemini-2.0-flash', api_key: '', gcp_project: '', sandbox_enabled: true },
  messages: [],
  tasks: {},
  current_session_id: null,
  sessions: {},
  session_history: [],
  usage_history: [],
  daemon: false,
  tokens: 0,
  tool_calls: 0,
  activeSandbox: null,
  zeroWorkspace: null,
};

export const MOCK_STORE_KEY = 'mowisai_mock_state_v2';

export function loadMockState() {
  try {
    const raw = localStorage.getItem(MOCK_STORE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    Object.assign(MockState, parsed);
  } catch {}
}

export function saveMockState() {
  try {
    localStorage.setItem(MOCK_STORE_KEY, JSON.stringify(MockState));
  } catch {}
}

// ── Mock invoke handlers ──────────────────────────────────────────────────────

export function mockInvoke(cmd, args) {
  switch (cmd) {
    case 'get_config':          return Promise.resolve(MockState.config);
    case 'save_config':         MockState.config = args.config; saveMockState(); return Promise.resolve();
    case 'get_sandbox_status':  return Promise.resolve(MockState.activeSandbox || null);
    case 'discard_sandbox':     MockState.activeSandbox = null; saveMockState(); return Promise.resolve();
    case 'get_sandbox_size':    return Promise.resolve(0);
    case 'get_zero_workspace':  return Promise.resolve(MockState.zeroWorkspace || null);
    case 'get_zero_workspace_base': return Promise.resolve('/mock/Documents/MowisAI/workspaces');
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

  const isZero = (mode || MockState.config.mode) === 'zero';

  // Simulate sandbox creation when enabled (not applicable in zero mode).
  MockState.activeSandbox = (!isZero && MockState.config.sandbox_enabled) ? {
    id: `sb-${Date.now().toString(36)}`,
    lower_dir: '/mock/project',
    upper_dir: `/tmp/mowis-sandbox/sb-${Date.now().toString(36)}/upper`,
  } : null;

  // Simulate zero workspace.
  const wsSlug = `mowis-${new Date().toISOString().slice(0,10).replace(/-/g,'')}-${Date.now().toString(36).slice(-6)}`;
  MockState.zeroWorkspace = isZero ? {
    session_id: id,
    slug: wsSlug,
    path: `/mock/Documents/MowisAI/workspaces/${wsSlug}`,
  } : null;
  MockState.sessions[id] = {
    summary: { id, prompt: prompt.slice(0, 80), status: 'running', started_at: nowTs(), completed_at: null, task_count: 0, tasks_done: 0 },
    messages: MockState.messages,
    tasks: [],
    tokens_total: 0,
    tool_calls_total: 0,
  };
  MockState.session_history.push(MockState.sessions[id].summary);
  saveMockState();

  // NOTE: Mock simulation has been PERMANENTLY REMOVED.
  // If you see this message, the Tauri API failed to load and the app is
  // running in browser-fallback mode. This should NEVER happen in a built app.
  console.error('[MowisAI] FATAL: start_session called in MOCK mode. Tauri API not loaded!');
  dispatchMockEvent('chat_message', {
    kind: 'agent',
    content: '**ERROR: Running in mock mode.** The Tauri backend is not connected. This build is broken — please rebuild with `cargo tauri build`.',
    ts: nowTs(),
  });
  mockSyncSession('error');
  return id;
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

// ── Simulation runners ────────────────────────────────────────────────────────

export async function runBrowserSimulation(sessionId, prompt, mode) {
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

export async function runZeroBrowserSimulation(sessionId, prompt, ws) {
  await delay(200);

  // Plan card — zero mode shows "zero" as the single sandbox.
  const plan = { kind: 'plan', sandboxes: ['zero'], task_count: 0, agent_count: 1, mode: 'zero', ts: nowTs() };
  MockState.messages.push(plan);
  mockSyncSession('running');
  dispatchMockEvent('chat_message', plan);

  await delay(300);

  // Announce workspace.
  const intro = [
    `**Zero-Protection mode** — writing directly to disk.\n`,
    `Workspace: \`${ws?.path || '/mock/MowisAI/workspaces/mowis-demo'}\`\n\n`,
    `Connecting to ${MockState.config.provider || 'gemini'} (${MockState.config.model || 'default model'})…\n`,
  ];
  for (const chunk of intro) {
    dispatchMockEvent('agent_chunk', { chunk });
    await delay(80);
  }

  // Simulate tool calls.
  const mockCalls = [
    { name: 'create_directory', desc: 'mkdir src/', file: 'src/' },
    { name: 'write_file', desc: 'Write src/main.py', file: 'src/main.py' },
    { name: 'write_file', desc: 'Write README.md', file: 'README.md' },
    { name: 'write_file', desc: 'Write requirements.txt', file: 'requirements.txt' },
    { name: 'run_command', desc: 'Run: echo "setup complete"', file: null },
    { name: 'list_directory', desc: 'List workspace', file: null },
  ];

  for (let i = 0; i < mockCalls.length; i++) {
    const call = mockCalls[i];
    const taskId = `z${String(i + 1).padStart(4, '0')}`;
    const task = {
      id: taskId,
      description: call.desc,
      sandbox: 'zero',
      status: 'pending',
      started_at: null,
      completed_at: nowTs(),
      files: call.file ? [call.file] : [],
      summary: `Executed ${call.name} in zero workspace.`,
      views: ['Workspace explorer'],
    };
    MockState.tasks[taskId] = task;
    dispatchMockEvent('task_added', task);
    await delay(40);

    MockState.tasks[taskId].status = 'running';
    MockState.tasks[taskId].started_at = nowTs();
    dispatchMockEvent('task_updated', { id: taskId, status: 'running' });
    await delay(180);

    MockState.tasks[taskId].status = 'complete';
    MockState.tasks[taskId].completed_at = nowTs();
    dispatchMockEvent('task_updated', { id: taskId, status: 'complete' });
    mockSyncSession('running');

    const echo = `\n\`${call.name}\` → ok${call.file ? ': ' + call.file : ''}\n`;
    dispatchMockEvent('agent_chunk', { chunk: echo });
    await delay(60);
  }

  const closing = [
    `\n\n**Done.** ${mockCalls.length} tool call(s) executed.\n`,
    `Files are saved at: \`${ws?.path || '/mock/MowisAI/workspaces/mowis-demo'}\`\n`,
  ];
  for (const chunk of closing) {
    dispatchMockEvent('agent_chunk', { chunk });
    await delay(100);
  }

  mockSyncSession('done');
  MockState.usage_history.push({
    session_id: sessionId,
    prompt_short: prompt.slice(0, 80),
    ts: nowTs(),
    task_count: mockCalls.length,
    tokens: 1200,
    tool_calls: mockCalls.length,
    duration_secs: 8,
    status: 'done',
  });
  saveMockState();
  dispatchMockEvent('session_complete', {});
}

// Initialize state on module load
loadMockState();
