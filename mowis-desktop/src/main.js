const AGENTS = [
  { id: 'a01', name: 'planner', task: 'Decomposing auth refactor into parallel workstreams.', sandbox: 'planner', status: 'running', step: '3 of 8', runtime: '00:14:22' },
  { id: 'a02', name: 'auth-1', task: 'Replacing legacy session middleware with rotating tokens.', sandbox: 'backend-α', status: 'running', step: '2 of 5', runtime: '00:08:11' },
  { id: 'a03', name: 'auth-2', task: 'Migrating user model for MFA enrollment.', sandbox: 'backend-β', status: 'running', step: '1 of 4', runtime: '00:03:42' },
  { id: 'a04', name: 'auth-3', task: 'Rewriting password hash to argon2id.', sandbox: 'backend-γ', status: 'running', step: '4 of 6', runtime: '00:11:08' },
  { id: 'a05', name: 'fe-login', task: 'Recomposing login form with new validation.', sandbox: 'frontend', status: 'running', step: '2 of 3', runtime: '00:06:55' },
  { id: 'a06', name: 'fe-2fa', task: 'Building OTP entry and fallback flow.', sandbox: 'frontend', status: 'running', step: '1 of 4', runtime: '00:02:18' },
  { id: 'a07', name: 'tests-unit', task: 'Generating unit tests for new session boundary.', sandbox: 'ci-α', status: 'running', step: '5 of 9', runtime: '00:18:00' },
  { id: 'a08', name: 'tests-int', task: 'Drafting integration tests across services.', sandbox: 'ci-β', status: 'running', step: '3 of 7', runtime: '00:12:30' },
  { id: 'a09', name: 'docs', task: 'Updating API reference for new endpoints.', sandbox: 'docs', status: 'running', step: '2 of 3', runtime: '00:04:11' },
  { id: 'a10', name: 'mig-db', task: 'Writing reversible migration for users table.', sandbox: 'db', status: 'running', step: '1 of 2', runtime: '00:01:42' },
  { id: 'a11', name: 'review-1', task: 'Reading diffs for security regressions.', sandbox: 'review', status: 'running', step: '—', runtime: '00:00:48' },
  { id: 'a12', name: 'review-2', task: 'Cross-checking naming consistency.', sandbox: 'review', status: 'running', step: '—', runtime: '00:00:32' },
  { id: 'b01', name: 'pool-1', task: 'Idle. Waiting for dispatch.', sandbox: 'pool', status: 'idle' },
  { id: 'b02', name: 'pool-2', task: 'Idle. Waiting for dispatch.', sandbox: 'pool', status: 'idle' },
  { id: 'c01', name: 'lint', task: 'Done — formatted backend module.', sandbox: 'ci-α', status: 'done' },
  { id: 'c02', name: 'audit', task: 'Done — license audit clean.', sandbox: 'review', status: 'done' }
];

const appState = {
  screen: 'session',
  sessionName: 'feature/auth-refactor',
  tasks: 8,
  sandboxes: 6,
  tokensPerHour: '184k',
  daemonConnected: true
};

const main = document.getElementById('main-content');
const grid = document.getElementById('agent-grid');
const statusbar = document.getElementById('statusbar');

function renderAgents() {
  const running = AGENTS.filter((a) => a.status === 'running');
  document.getElementById('running-count').textContent = running.length;
  document.getElementById('agent-ratio').textContent = `${running.length}/${AGENTS.length}`;

  grid.innerHTML = '';
  AGENTS.slice(0, 16).forEach((agent) => {
    const tile = document.createElement('div');
    tile.className = `mw-agent-tile ${agent.status}`;
    tile.innerHTML = `<span class="id">${agent.id}</span>`;

    const pop = document.createElement('div');
    pop.className = 'mw-agent-pop';
    pop.innerHTML = `
      <div class="name">${agent.id.toUpperCase()} · ${agent.name}</div>
      <div class="task">${agent.task}</div>
      <div class="row"><span>Sandbox</span><b>${agent.sandbox}</b></div>
      <div class="row"><span>Step</span><b>${agent.step || '—'}</b></div>
      <div class="row"><span>Status</span><b>${agent.status}</b></div>
      ${agent.runtime ? `<div class="row"><span>Running</span><b>${agent.runtime}</b></div>` : ''}
    `;

    tile.addEventListener('mouseenter', () => tile.appendChild(pop));
    tile.addEventListener('mouseleave', () => pop.remove());
    tile.addEventListener('click', () => {
      appState.screen = 'agent';
      setActiveNav('agent');
      renderScreen();
    });

    grid.appendChild(tile);
  });
}

function renderStatusBar() {
  statusbar.innerHTML = `
    <span><span class="mw-status-dot" style="margin-right:8px"></span><b>${appState.daemonConnected ? 'Connected' : 'Disconnected'}</b></span>
    <span class="sep">·</span>
    <span><b>${AGENTS.filter((a) => a.status === 'running').length}</b> agents running</span>
    <span class="sep">·</span>
    <span><b>${appState.sandboxes}</b> sandboxes</span>
    <span class="sep">·</span>
    <span><b>${appState.tokensPerHour}</b> tokens / hr</span>
    <span style="margin-left:auto">main · clean · ⌘K</span>
  `;
}

function templateWelcome() {
  return `<div class="mw-welcome" style="height:100%"><div class="word" style="filter: blur(2px)">Welcome to MowisAI</div><div class="continue"><button id="continue-btn">continue</button></div></div>`;
}

function templateHome() {
  return `
    <div class="mw-spawn">
      <div class="mw-section-label" style="padding:0;margin:0">A new task</div>
      <h1 style="font-style:italic">What should we work on?</h1>
      <div class="lede">Describe a goal and MowisAI will decompose work across sandboxes and agents.</div>
      <div class="mw-spawn-input">
        <textarea id="task-input" placeholder="Refactor authentication layer, add MFA, update docs and tests."></textarea>
        <div class="controls">
          <span style="font-family:var(--sans);font-size:10px;color:var(--tx-3);letter-spacing:.08em;text-transform:uppercase">Repo · mowis/api</span>
          <span style="font-family:var(--sans);font-size:10px;color:var(--tx-3);letter-spacing:.08em;text-transform:uppercase">Branch · main</span>
          <span style="flex:1"></span>
          <button class="mw-btn ghost">Plan first</button>
          <button class="mw-btn primary" id="run-task">↩</button>
        </div>
      </div>
    </div>`;
}

function templateSession() {
  return `
    <div class="mw-chat">
      <div class="turn"><div class="role">You · 14:08</div><div class="body">Refactor authentication for MFA and rotating tokens. Keep tests green and docs updated.</div></div>
      <div class="system"><span class="tag">PLAN</span><span>Planner created 8 tasks · 6 sandboxes · 12 agents dispatched</span></div>
      <div class="turn"><div class="role">Planner · 14:09</div><div class="body muted">Three tracks run in parallel first: token middleware, hash migration, OTP UI. Dependencies auto-unblock via scheduler.</div></div>
      <div class="system"><span class="tag">DISPATCH</span><span>auth-1 auth-2 auth-3 → backend · fe-login fe-2fa → frontend · mig-db → db</span></div>
      <div class="turn"><div class="role">auth-1 · 14:14</div><div class="body">Token rotation middleware complete. Asking whether to enforce per-device revocation now or in follow-up.</div></div>
      <div class="mw-composer">
        <textarea placeholder="Type instructions to the fleet..."></textarea>
        <div class="controls">
          <span class="chip">Planner</span><span class="chip">Broadcast</span><span class="chip">Checkpoint every tool call</span>
          <span style="flex:1"></span>
          <button class="mw-btn ghost">Pause</button><button class="mw-btn primary">Send</button>
        </div>
      </div>
    </div>`;
}

function templateAgentDetail() {
  const a = AGENTS[1];
  return `
    <div class="mw-page-head"><div><div class="kicker">Agent ${a.id}</div><h1>${a.name}</h1></div><div class="mw-btn ghost">Reassign</div></div>
    <div class="mw-page-body">
      <div class="mw-grid-2">
        <div class="mw-card"><div class="kicker">Task</div><p>${a.task}</p></div>
        <div class="mw-card"><div class="kicker">Runtime</div><p>${a.runtime}</p><div class="kicker">Step ${a.step}</div></div>
      </div>
      <div class="mw-code">diff --git a/auth.rs b/auth.rs\n+ rotate_token(req.user_id);\n+ write_audit_log("token rotated");\n- legacy_session_extend();</div>
    </div>`;
}

function templateSandboxes() {
  return `
    <div class="mw-page-head"><div><div class="kicker">Execution</div><h1>Sandboxes</h1></div><button class="mw-btn primary">+ New sandbox</button></div>
    <div class="mw-page-body">
      <div class="mw-grid-3">
        <article class="mw-card"><div class="kicker">backend-α</div><h3>Rust API</h3><p>4 agents · running</p></article>
        <article class="mw-card"><div class="kicker">frontend</div><h3>Tauri UI</h3><p>2 agents · running</p></article>
        <article class="mw-card"><div class="kicker">db</div><h3>Postgres migration</h3><p>1 agent · running</p></article>
        <article class="mw-card"><div class="kicker">ci-α</div><h3>Unit tests</h3><p>2 agents · running</p></article>
        <article class="mw-card"><div class="kicker">ci-β</div><h3>Integration tests</h3><p>1 agent · running</p></article>
        <article class="mw-card"><div class="kicker">review</div><h3>Merge review</h3><p>2 agents · running</p></article>
      </div>
    </div>`;
}

function templateTimeline() {
  return `
    <div class="mw-page-head"><div><div class="kicker">Observability</div><h1>Timeline</h1></div></div>
    <div class="mw-page-body">
      <div class="mw-timeline">
        <div class="item"><span>14:08</span><div>Session started from Home prompt.</div></div>
        <div class="item"><span>14:09</span><div>Planner emitted task graph and topology.</div></div>
        <div class="item"><span>14:10</span><div>Agents spawned in backend/frontend/db sandboxes.</div></div>
        <div class="item"><span>14:14</span><div>auth-1 requested policy clarification.</div></div>
        <div class="item"><span>14:16</span><div>tests-unit passed initial checkpoint.</div></div>
      </div>
    </div>`;
}

function templateSettings() {
  return `
    <div class="mw-page-head"><div><div class="kicker">Preferences</div><h1>Settings</h1></div></div>
    <div class="mw-page-body">
      <div class="mw-grid-2">
        <div class="mw-card"><div class="kicker">Runtime</div><label><input id="daemon-toggle" type="checkbox" ${appState.daemonConnected ? 'checked' : ''}/> Daemon connected</label></div>
        <div class="mw-card"><div class="kicker">Theme</div><label>Accent hue <input id="hue" type="range" min="180" max="260" value="260"/></label></div>
      </div>
    </div>`;
}

const templates = {
  welcome: templateWelcome,
  home: templateHome,
  session: templateSession,
  agent: templateAgentDetail,
  sandboxes: templateSandboxes,
  timeline: templateTimeline,
  settings: templateSettings
};

function renderScreen() {
  document.getElementById('screen-label').textContent = appState.screen[0].toUpperCase() + appState.screen.slice(1);
  document.getElementById('session-label').textContent = appState.sessionName;
  main.innerHTML = templates[appState.screen]();
  wireScreenEvents();
}

function wireScreenEvents() {
  document.getElementById('continue-btn')?.addEventListener('click', () => {
    appState.screen = 'home';
    setActiveNav('home');
    renderScreen();
  });

  document.getElementById('run-task')?.addEventListener('click', () => {
    const txt = document.getElementById('task-input').value.trim();
    if (txt) appState.sessionName = txt.slice(0, 28);
    appState.screen = 'session';
    setActiveNav('session');
    renderScreen();
  });

  document.getElementById('daemon-toggle')?.addEventListener('change', (e) => {
    appState.daemonConnected = e.target.checked;
    document.getElementById('daemon-status').textContent = appState.daemonConnected ? 'DAEMON' : 'OFFLINE';
    renderStatusBar();
  });

  document.getElementById('hue')?.addEventListener('input', (e) => {
    const hue = e.target.value;
    document.documentElement.style.setProperty('--blue', `hsl(${hue}, 100%, 60%)`);
    document.documentElement.style.setProperty('--blue-glow', `hsla(${hue}, 100%, 60%, 0.55)`);
  });
}

function setActiveNav(id) {
  document.querySelectorAll('.mw-nav-item').forEach((node) => node.classList.toggle('active', node.dataset.screen === id));
}

document.querySelectorAll('.mw-nav-item').forEach((item) => {
  item.addEventListener('click', () => {
    const screen = item.dataset.screen;
    appState.screen = screen;
    setActiveNav(screen);
    renderScreen();
  });
});

async function hydrateFromTauri() {
  if (!window.__TAURI__?.core?.invoke) return;
  try {
    const payload = await window.__TAURI__.core.invoke('app_health');
    document.getElementById('build-version').textContent = payload.version;
    appState.daemonConnected = payload.daemon_connected;
    appState.tokensPerHour = payload.tokens_per_hour;
    renderStatusBar();
  } catch (e) {
    console.warn('Failed to invoke tauri command', e);
  }
}

renderAgents();
renderScreen();
renderStatusBar();
hydrateFromTauri();
