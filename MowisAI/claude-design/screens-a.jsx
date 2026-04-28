/* global React */
// MowisAI — Variation A (Quiet Editorial)
// All screens for the desktop app. Each screen exports a component used
// inside <DCArtboard> on the main canvas.

const { useState, useEffect, useMemo } = React;

// ─── Shared atoms ────────────────────────────────────────────
function TitleBar({ crumb = ['MowisAI', 'Session', 'feature/auth-refactor'] }) {
  return (
    <div className="mw-titlebar">
      <div className="mw-traffic"><span /><span /><span /></div>
      <div className="mw-titlebar-center">
        {crumb.map((c, i) =>
        <React.Fragment key={i}>
            {i > 0 && <span className="sep">/</span>}
            <span style={{ fontStyle: i === crumb.length - 1 ? 'italic' : 'normal' }}>{c}</span>
          </React.Fragment>
        )}
      </div>
      <div className="mw-titlebar-right">
        <span><span className="mw-status-dot" />DAEMON</span>
        <span>v0.4.2</span>
      </div>
    </div>);

}

function StatusBar({ agents = 12, sandboxes = 4, tokens = '184k' }) {
  return (
    <div className="mw-statusbar">
      <span><span className="mw-status-dot" style={{ marginRight: 8 }} /><b>Connected</b></span>
      <span className="sep">·</span>
      <span><b>{agents}</b> agents running</span>
      <span className="sep">·</span>
      <span><b>{sandboxes}</b> sandboxes</span>
      <span className="sep">·</span>
      <span><b>{tokens}</b> tokens / hr</span>
      <span style={{ marginLeft: 'auto' }}>main · clean · ⌘K</span>
    </div>);

}

function Sidebar({ active = 'session' }) {
  const items = [
  { id: 'home', label: 'Home' },
  { id: 'session', label: 'Active session', italic: true },
  { id: 'agents', label: 'Agents', badge: '12' },
  { id: 'sandboxes', label: 'Sandboxes', badge: '4' },
  { id: 'timeline', label: 'Timeline' }];

  const lib = [
  { id: 'sessions', label: 'Past sessions' },
  { id: 'files', label: 'Files' }];

  return (
    <aside className="mw-sidebar">
      <div className="mw-brand">Mowis<span style={{ fontStyle: 'italic' }}>AI</span><span className="dot" /></div>
      <div className="mw-section-label">Workspace</div>
      {items.map((it) =>
      <div key={it.id} className={`mw-nav-item ${it.id === active ? 'active' : ''}`}>
          <span style={{ fontStyle: it.italic ? 'italic' : 'normal' }}>{it.label}</span>
          {it.badge && <span className="badge">{it.badge}</span>}
        </div>
      )}
      <div className="mw-section-label">Library</div>
      {lib.map((it) =>
      <div key={it.id} className="mw-nav-item">
          <span>{it.label}</span>
        </div>
      )}
      <div className="mw-sidebar-spacer" />
      <div className="mw-nav-item"><span>Settings</span></div>
      <div className="mw-sidebar-foot">
        you@mowis.ai<br />
        Pro · 320 hrs left
      </div>
    </aside>);

}

// Agent tile data — used in panel & dashboards.
const AGENTS = [
{ id: 'a01', name: 'planner', task: 'Decomposing the auth refactor into discrete tasks across services.', sandbox: 'planner', status: 'running', step: '3 of 8', for: '00:14:22' },
{ id: 'a02', name: 'auth-1', task: 'Replacing legacy session middleware with token rotation logic.', sandbox: 'backend-α', status: 'running', step: '2 of 5', for: '00:08:11' },
{ id: 'a03', name: 'auth-2', task: 'Migrating user model to support multi-factor enrolment.', sandbox: 'backend-β', status: 'running', step: '1 of 4', for: '00:03:42' },
{ id: 'a04', name: 'auth-3', task: 'Rewriting password hash function to argon2id.', sandbox: 'backend-γ', status: 'running', step: '4 of 6', for: '00:11:08' },
{ id: 'a05', name: 'fe-login', task: 'Recomposing the login form with new field validation.', sandbox: 'frontend', status: 'running', step: '2 of 3', for: '00:06:55' },
{ id: 'a06', name: 'fe-2fa', task: 'Building the OTP entry surface and fallback flows.', sandbox: 'frontend', status: 'running', step: '1 of 4', for: '00:02:18' },
{ id: 'a07', name: 'tests-unit', task: 'Generating unit tests for the new session boundary.', sandbox: 'ci-α', status: 'running', step: '5 of 9', for: '00:18:00' },
{ id: 'a08', name: 'tests-int', task: 'Drafting integration tests across services.', sandbox: 'ci-β', status: 'running', step: '3 of 7', for: '00:12:30' },
{ id: 'a09', name: 'docs', task: 'Updating API reference for new endpoints.', sandbox: 'docs', status: 'running', step: '2 of 3', for: '00:04:11' },
{ id: 'a10', name: 'mig-db', task: 'Writing reversible migration for users table.', sandbox: 'db', status: 'running', step: '1 of 2', for: '00:01:42' },
{ id: 'a11', name: 'review-1', task: 'Reading diff for security regressions.', sandbox: 'review', status: 'running', step: '—', for: '00:00:48' },
{ id: 'a12', name: 'review-2', task: 'Cross-checking style and naming consistency.', sandbox: 'review', status: 'running', step: '—', for: '00:00:32' },
{ id: 'b01', name: 'idle', task: 'Waiting for planner to dispatch.', sandbox: 'pool', status: 'idle' },
{ id: 'b02', name: 'idle', task: 'Waiting for planner to dispatch.', sandbox: 'pool', status: 'idle' },
{ id: 'b03', name: 'idle', task: 'Waiting for planner to dispatch.', sandbox: 'pool', status: 'idle' },
{ id: 'b04', name: 'idle', task: 'Waiting for planner to dispatch.', sandbox: 'pool', status: 'idle' },
{ id: 'c01', name: 'lint', task: 'Resolved formatting on backend module.', sandbox: 'ci-α', status: 'done' },
{ id: 'c02', name: 'fmt', task: 'Done — applied black formatting on Python services.', sandbox: 'ci-α', status: 'done' },
{ id: 'c03', name: 'audit', task: 'Done — license audit clean.', sandbox: 'review', status: 'done' },
{ id: 'c04', name: 'depbump', task: 'Done — bumped 4 minor versions.', sandbox: 'ci-β', status: 'done' }];


function AgentsPanel({ side = 'right', density = 'sparse', count = 12 }) {
  const [hovered, setHovered] = useState(null);
  // Show running first, idle, then done — based on `count` running.
  const running = AGENTS.filter((a) => a.status === 'running').slice(0, count);
  const idle = AGENTS.filter((a) => a.status === 'idle');
  const done = AGENTS.filter((a) => a.status === 'done');
  const all = [...running, ...idle, ...done];
  // Density tweaks # of tiles shown.
  const cap = density === 'dense' ? 24 : density === 'balanced' ? 18 : 14;
  const list = all.slice(0, cap);
  const totalRunning = running.length;
  const popped = hovered != null ? list[hovered] : null;
  return (
    <aside className="mw-agents" style={side === 'left' ? { borderLeft: 'none', borderRight: '1px solid var(--line)', order: -1 } : {}}>
      <div className="mw-agents-head">
        <div className="title">
          <span style={{ fontStyle: 'italic' }}>Agents</span>
          <span style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.04em' }}>
            {totalRunning}/{AGENTS.length}
          </span>
        </div>
        <div className="meta">Fleet · live</div>
        <div className="count-row">
          <div className="count">{totalRunning}</div>
          <div className="count-label">Running<br />this moment</div>
        </div>
      </div>
      <div className="mw-agent-grid">
        {list.map((a, i) =>
        <div
          key={a.id + i}
          className={`mw-agent-tile ${a.status}`}
          onMouseEnter={() => setHovered(i)}
          onMouseLeave={() => setHovered(null)}>
          
            <span className="id">{a.id}</span>
            {hovered === i &&
          <div className="mw-agent-pop">
                <div className="name">{a.id.toUpperCase()} · {a.name}</div>
                <div className="task">{a.task}</div>
                <div className="row"><span>Sandbox</span><b>{a.sandbox}</b></div>
                {a.step && <div className="row"><span>Step</span><b>{a.step}</b></div>}
                {a.for && <div className="row"><span>Running</span><b>{a.for}</b></div>}
                <div className="row"><span>Status</span><b style={{ color: a.status === 'running' ? 'var(--blue)' : 'var(--tx-2)' }}>{a.status}</b></div>
              </div>
          }
          </div>
        )}
      </div>
    </aside>);

}

// ─── Welcome screen ──────────────────────────────────────────
function WelcomeScreen({ blur = 70 }) {
  const [b, setB] = useState(blur * 0.2);
  useEffect(() => {
    const t = setTimeout(() => setB(blur * 0.02), 200);
    return () => clearTimeout(t);
  }, [blur]);
  return (
    <div style={{ width: '100%', height: '100%', background: '#000', display: 'flex', flexDirection: 'column', borderRadius: 12, overflow: 'hidden', position: 'relative' }}>
      <div className="mw-titlebar" style={{ background: 'transparent', borderBottom: 'none' }}>
        <div className="mw-traffic"><span /><span /><span /></div>
      </div>
      <div className="mw-welcome">
        <div className="word" style={{ filter: `blur(${b}px)`, transition: 'filter 3.2s cubic-bezier(.2,.7,.2,1)' }}>
          Welcome to MowisAI
        </div>
        <div className="continue">
          <button>continue</button>
        </div>
      </div>
    </div>);

}

// ─── Home / Landing ──────────────────────────────────────────
function HomeScreen() {
  const recent = [
  { name: 'feature/auth-refactor', time: 'today · 2h ago', repo: 'mowis/api', agents: 12 },
  { name: 'fix/payments-webhook', time: 'yesterday', repo: 'mowis/api', agents: 6 },
  { name: 'refactor/onboarding-flow', time: 'Mon, Apr 22', repo: 'mowis/web', agents: 18 },
  { name: 'spike/embedding-eval', time: 'Apr 20', repo: 'mowis/research', agents: 4 }];

  return (
    <div className="mw-app">
      <TitleBar crumb={['MowisAI', 'Home']} />
      <div className="mw-body">
        <Sidebar active="home" />
        <main className="mw-main">
          <div className="mw-spawn">
            <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>A new task</div>
            <h1 style={{ fontStyle: 'italic' }}>What should we work on?</h1>
            <div className="lede">Describe a goal. The planner will decompose it into parallel tasks and dispatch agents across your sandboxes.</div>
            <div className="mw-spawn-input">
              <textarea placeholder="Refactor the authentication layer to support multi-factor and rotating tokens, then update the docs and tests." />
              <div className="controls">
                <span className="mw-composer .chip" style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Repo · mowis/api</span>
                <span style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.06em', textTransform: 'uppercase' }}>Branch · main</span>
                <span style={{ flex: 1 }} />
                <button className="mw-btn ghost">Plan first</button>
                <button className="mw-btn primary">↩</button>
              </div>
            </div>
            <div style={{ width: '100%', maxWidth: 720, marginTop: 14 }}>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Recent</div>
              {recent.map((r, i) =>
              <div key={i} style={{ display: 'flex', alignItems: 'baseline', gap: 18, padding: '12px 0', borderBottom: '1px solid var(--line)' }}>
                  <div style={{ fontFamily: 'var(--serif)', fontSize: 14, fontStyle: 'italic', color: 'var(--tx)' }}>{r.name}</div>
                  <div style={{ fontFamily: 'var(--mono)', fontSize: 11, color: 'var(--tx-3)' }}>{r.repo}</div>
                  <div style={{ flex: 1 }} />
                  <div style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.10em', textTransform: 'uppercase' }}>{r.agents} agents · {r.time}</div>
                </div>
              )}
            </div>
          </div>
        </main>
      </div>
      <StatusBar agents={0} sandboxes={0} tokens="0" />
    </div>);

}

// ─── Active Session (the heart) ──────────────────────────────
function SessionScreen({ panelSide = 'right', density = 'sparse', agentCount = 12 }) {
  return (
    <div className="mw-app">
      <TitleBar />
      <div className="mw-body">
        <Sidebar active="session" />
        <main className="mw-main">
          <div className="mw-chat">
            <div className="turn">
              <div className="role">You · 14:08</div>
              <div className="body">
                Refactor the authentication layer to support multi-factor and rotating tokens. Update the docs and tests as you go. Coordinate across <span style={{ fontStyle: 'italic' }}>backend</span>, <span style={{ fontStyle: 'italic' }}>frontend</span>, and <span style={{ fontStyle: 'italic' }}>db</span>.
              </div>
            </div>
            <div className="system">
              <span className="tag">PLAN</span>
              <span>Planner produced 8 tasks · 4 sandboxes opened · 12 agents dispatched</span>
            </div>
            <div className="turn">
              <div className="role">Planner · 14:09</div>
              <div className="body muted">
                I broke the work into eight tasks. Three of them are independent and can run in parallel: rotating-token middleware, argon2id hash migration, and the OTP frontend. Two depend on the migration finishing first. I'll keep the review pair watching all branches and surface anything that drifts from the existing style.
              </div>
            </div>
            <div className="system">
              <span className="tag">DISPATCH</span>
              <span>auth-1, auth-2, auth-3 → backend · fe-login, fe-2fa → frontend · mig-db → db</span>
            </div>
            <div className="turn">
              <div className="role">auth-1 · 14:14</div>
              <div className="body muted">
                Replaced the legacy <span style={{ fontFamily: 'var(--mono)', fontSize: 13 }}>SessionMiddleware</span> with a rotating-token boundary. The new boundary issues a short-lived access token and a refresh token bound to device fingerprint. Tests for the previous behaviour pass against the new contract. Awaiting review.
              </div>
            </div>
            <div className="system">
              <span className="tag">REVIEW</span>
              <span>review-1 flagged: refresh token TTL exceeds policy · awaiting your input</span>
            </div>
          </div>
          <div className="mw-composer">
            <textarea className="input" placeholder="Reply, redirect, or @mention an agent…" />
            <div className="row">
              <span className="chip">@auth-1</span>
              <span className="chip">file: api/auth.py</span>
              <span style={{ flex: 1 }} />
              <button className="mw-btn ghost">Pause all</button>
              <button className="mw-btn primary send">Send</button>
            </div>
          </div>
          <div className="mw-diff">
            <div className="mw-diff-head">
              <span className="file active">api/auth.py</span>
              <span className="file">api/users.py</span>
              <span className="file">web/login.tsx</span>
              <span style={{ marginLeft: 18, color: 'var(--tx-4)' }}>+ 4 more</span>
              <div className="actions">
                <button className="mw-btn ghost">Reject</button>
                <button className="mw-btn primary">Accept all</button>
              </div>
            </div>
            <div className="mw-diff-body">
              <div className="ctx"><span className="marker"> </span>def authenticate(request):</div>
              <div className="del"><span className="marker">−</span>    session = SessionMiddleware.get(request)</div>
              <div className="del"><span className="marker">−</span>    if not session or session.expired:</div>
              <div className="del"><span className="marker">−</span>        return None</div>
              <div className="add"><span className="marker">+</span>    token = TokenBoundary.read(request)</div>
              <div className="add"><span className="marker">+</span>    if not token.valid_for(request.fingerprint):</div>
              <div className="add"><span className="marker">+</span>        return Refresh.try_rotate(token, request)</div>
              <div className="ctx"><span className="marker"> </span>    return token.user</div>
            </div>
          </div>
        </main>
        <AgentsPanel side={panelSide} density={density} count={agentCount} />
      </div>
      <StatusBar />
    </div>);

}

// ─── Single Agent Detail ─────────────────────────────────────
function AgentDetailScreen() {
  const log = [
  { t: '14:08:42', text: 'Spawned in sandbox backend-α from planner dispatch.' },
  { t: '14:08:51', text: 'Cloned api repo at sha 9f8b2c3.' },
  { t: '14:09:02', text: 'Opened api/auth.py and api/middleware/session.py for reading.' },
  { t: '14:09:31', text: 'Drafted token boundary contract; checked existing tests.' },
  { t: '14:11:14', text: 'Wrote api/auth/token_boundary.py — rotating tokens, fingerprint-bound.' },
  { t: '14:12:08', text: 'Replaced 14 references to SessionMiddleware across 6 files.' },
  { t: '14:13:55', text: 'Ran unit tests · 47 passed · 2 failed in oauth flow.' },
  { t: '14:14:18', text: 'Asked review-1 for security read on refresh TTL.' },
  { t: '14:14:31', text: 'Awaiting human input on TTL exceeding policy.' }];

  return (
    <div className="mw-app">
      <TitleBar crumb={['MowisAI', 'Session', 'agents', 'auth-1']} />
      <div className="mw-body">
        <Sidebar active="agents" />
        <main className="mw-main">
          <div className="mw-page-head">
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>A02 · Agent</div>
              <h1><span style={{ fontStyle: 'italic' }}>auth-1</span></h1>
              <div className="sub">Sandbox backend-α · running 14m 22s</div>
            </div>
            <div style={{ display: 'flex', gap: 10 }}>
              <button className="mw-btn ghost">Pause</button>
              <button className="mw-btn ghost">Fork</button>
              <button className="mw-btn">Open sandbox</button>
            </div>
          </div>
          <div className="mw-detail">
            <section>
              <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>Current task</div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 22, color: 'var(--tx)', lineHeight: 1.35, margin: '8px 0 0', letterSpacing: '-0.005em' }}>
                Replace legacy session middleware with a rotating-token boundary that binds refresh to device fingerprint.
              </div>
              <dl className="mw-kv">
                <dt>Status</dt><dd><span style={{ color: 'var(--blue)', fontStyle: 'italic' }}>awaiting input</span></dd>
                <dt>Step</dt><dd>2 of 5 · Review block</dd>
                <dt>Sandbox</dt><dd className="mono">backend-α · py-3.12</dd>
                <dt>Branch</dt><dd className="mono">refactor/auth/token-boundary</dd>
                <dt>Spawned</dt><dd>14:08:42 by planner</dd>
                <dt>Cost</dt><dd>$1.84 · 92k tokens</dd>
                <dt>Files</dt><dd className="mono">7 changed · +218 / −94</dd>
              </dl>
              <div className="mw-section-label" style={{ padding: 0, margin: '28px 0 8px' }}>Question for you</div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 15, color: 'var(--tx-2)', lineHeight: 1.6, fontStyle: 'italic' }}>
                "The refresh token TTL I picked (30 days) exceeds the policy in <span style={{ fontFamily: 'var(--mono)', fontStyle: 'normal', fontSize: 13 }}>SECURITY.md</span> (14 days). Should I tighten to 14 days, or amend the policy first?"
              </div>
              <div style={{ display: 'flex', gap: 10, marginTop: 14 }}>
                <button className="mw-btn">Tighten to 14d</button>
                <button className="mw-btn">Amend policy</button>
                <button className="mw-btn ghost">Reply…</button>
              </div>
            </section>
            <section>
              <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>Activity</div>
              <div className="mw-timeline" style={{ marginTop: 10 }}>
                {log.map((l, i) =>
                <div className="row" key={i} style={{ gridTemplateColumns: '70px 1fr' }}>
                    <div className="time">{l.t}</div>
                    <div className="text">{l.text}</div>
                  </div>
                )}
              </div>
            </section>
          </div>
        </main>
      </div>
      <StatusBar />
    </div>);

}

// ─── Spawn / New Task ────────────────────────────────────────
function SpawnScreen() {
  return (
    <div className="mw-app">
      <TitleBar crumb={['MowisAI', 'New task']} />
      <div className="mw-body">
        <Sidebar active="home" />
        <main className="mw-main">
          <div className="mw-spawn">
            <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>New dispatch</div>
            <h1><span style={{ fontStyle: 'italic' }}>What should we work on?</span></h1>
            <div className="lede">
              Describe a goal in plain language. The planner decomposes it; the fleet dispatches in parallel; you stay in the loop only where it matters.
            </div>
            <div className="mw-spawn-input">
              <textarea placeholder="Refactor the authentication layer to support multi-factor and rotating tokens, then update the docs and tests." />
              <div className="controls">
                <span style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.10em', textTransform: 'uppercase' }}>Repo · mowis/api</span>
                <span style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.10em', textTransform: 'uppercase' }}>Branch · main</span>
                <span style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.10em', textTransform: 'uppercase' }}>Fleet · auto</span>
                <span style={{ flex: 1 }} />
                <button className="mw-btn ghost">Plan only</button>
                <button className="mw-btn primary">↩</button>
              </div>
            </div>
            <div className="mw-spawn-suggestions">
              <span className="mw-suggestion"><span style={{ fontStyle: 'italic' }}>Audit</span> the codebase for n+1 queries</span>
              <span className="mw-suggestion"><span style={{ fontStyle: 'italic' }}>Migrate</span> the worker queue to Redis Streams</span>
              <span className="mw-suggestion"><span style={{ fontStyle: 'italic' }}>Generate</span> integration tests for the billing service</span>
              <span className="mw-suggestion"><span style={{ fontStyle: 'italic' }}>Translate</span> the marketing site to French and German</span>
            </div>
          </div>
        </main>
      </div>
      <StatusBar agents={0} sandboxes={0} tokens="0" />
    </div>);

}

// ─── Sandboxes panel ─────────────────────────────────────────
function SandboxesScreen() {
  const sb = [
  { name: 'backend-α', tag: 'py-3.12 · 4 vCPU · 8 GB', agents: 4, ram: 0.62, cpu: 0.41, status: 'running' },
  { name: 'backend-β', tag: 'py-3.12 · 2 vCPU · 4 GB', agents: 2, ram: 0.34, cpu: 0.22, status: 'running' },
  { name: 'frontend', tag: 'node-20 · 4 vCPU · 8 GB', agents: 3, ram: 0.48, cpu: 0.55, status: 'running' },
  { name: 'db', tag: 'pg-16 · 2 vCPU · 4 GB', agents: 1, ram: 0.18, cpu: 0.11, status: 'running' },
  { name: 'ci-α', tag: 'ubuntu-22 · 8 vCPU · 16 GB', agents: 1, ram: 0.21, cpu: 0.30, status: 'running' },
  { name: 'review', tag: 'observer · 1 vCPU · 2 GB', agents: 2, ram: 0.10, cpu: 0.04, status: 'running' }];

  return (
    <div className="mw-app">
      <TitleBar crumb={['MowisAI', 'Session', 'sandboxes']} />
      <div className="mw-body">
        <Sidebar active="sandboxes" />
        <main className="mw-main">
          <div className="mw-page-head">
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Environments</div>
              <h1><span style={{ fontStyle: 'italic' }}>Sandboxes</span></h1>
              <div className="sub">6 running · 13 agents attached</div>
            </div>
            <button className="mw-btn">New sandbox</button>
          </div>
          <div className="mw-cards">
            {sb.map((s, i) =>
            <div key={i} className="mw-card">
                <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between' }}>
                  <div className="name"><span style={{ fontStyle: 'italic' }}>{s.name}</span></div>
                  <div style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--blue)', letterSpacing: '0.10em', textTransform: 'uppercase' }}>· {s.status}</div>
                </div>
                <div className="meta">{s.tag}</div>
                <div style={{ display: 'flex', gap: 32, marginTop: 'auto' }}>
                  <div style={{ flex: 1 }}>
                    <div className="row"><span>CPU</span><b>{Math.round(s.cpu * 100)}%</b></div>
                    <div className="bar" style={{ marginTop: 6 }}><span style={{ width: `${s.cpu * 100}%` }} /></div>
                  </div>
                  <div style={{ flex: 1 }}>
                    <div className="row"><span>Memory</span><b>{Math.round(s.ram * 100)}%</b></div>
                    <div className="bar" style={{ marginTop: 6 }}><span style={{ width: `${s.ram * 100}%`, background: 'rgba(255,255,255,0.4)' }} /></div>
                  </div>
                  <div>
                    <div className="row"><span>Agents</span><b>{s.agents}</b></div>
                  </div>
                </div>
              </div>
            )}
          </div>
        </main>
      </div>
      <StatusBar />
    </div>);

}

// ─── Activity Timeline ───────────────────────────────────────
function TimelineScreen() {
  const events = [
  { t: '14:14:31', who: 'auth-1', text: 'asked for input on refresh token TTL.', tag: 'block' },
  { t: '14:14:18', who: 'review-1', text: 'flagged a security policy mismatch on auth-1.', tag: 'review' },
  { t: '14:13:55', who: 'auth-1', text: 'ran the auth unit suite — 47 passed, 2 failing.', tag: 'tests' },
  { t: '14:12:08', who: 'auth-1', text: 'replaced 14 references to SessionMiddleware across 6 files.', tag: 'edit' },
  { t: '14:11:42', who: 'fe-login', text: 'rendered new validation states for the login form.', tag: 'edit' },
  { t: '14:11:14', who: 'auth-1', text: 'wrote api/auth/token_boundary.py.', tag: 'edit' },
  { t: '14:10:33', who: 'mig-db', text: 'wrote a reversible migration for the users table.', tag: 'db' },
  { t: '14:09:21', who: 'planner', text: 'dispatched 12 agents across 4 sandboxes.', tag: 'plan' },
  { t: '14:09:02', who: 'planner', text: 'produced 8 tasks from your goal.', tag: 'plan' },
  { t: '14:08:42', who: 'you', text: 'started a session: refactor the authentication layer.', tag: 'start' }];

  return (
    <div className="mw-app">
      <TitleBar crumb={['MowisAI', 'Session', 'timeline']} />
      <div className="mw-body">
        <Sidebar active="timeline" />
        <main className="mw-main">
          <div className="mw-page-head">
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Session log</div>
              <h1><span style={{ fontStyle: 'italic' }}>Timeline</span></h1>
              <div className="sub">since 14:08 · live</div>
            </div>
            <div style={{ display: 'flex', gap: 10 }}>
              <button className="mw-btn ghost">All agents</button>
              <button className="mw-btn ghost">Filter</button>
            </div>
          </div>
          <div className="mw-page-body">
            <div className="mw-timeline">
              {events.map((e, i) =>
              <div className="row" key={i}>
                  <div className="time">{e.t}</div>
                  <div className="text"><span className="who">{e.who}</span> {e.text}</div>
                  <div className="meta">{e.tag}</div>
                </div>
              )}
            </div>
          </div>
        </main>
      </div>
      <StatusBar />
    </div>);

}

// ─── Settings ────────────────────────────────────────────────
function SettingsScreen() {
  return (
    <div className="mw-app">
      <TitleBar crumb={['MowisAI', 'Settings']} />
      <div className="mw-body">
        <Sidebar active="settings" />
        <main className="mw-main">
          <div className="mw-page-head">
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Preferences</div>
              <h1><span style={{ fontStyle: 'italic' }}>Settings</span></h1>
              <div className="sub">applies to this workspace · you@mowis.ai</div>
            </div>
          </div>
          <div className="mw-page-body" style={{ maxWidth: 820 }}>
            <div className="mw-form-row">
              <div className="label">API key
                <span className="hint">Used by the local daemon to dispatch agents. Never leaves this machine.</span>
              </div>
              <div className="control">
                <input className="mw-input" defaultValue="mw_live_••••••••••••••••••••••••2f7c" />
                <div style={{ display: 'flex', gap: 8 }}>
                  <button className="mw-btn ghost">Reveal</button>
                  <button className="mw-btn ghost">Rotate</button>
                </div>
              </div>
            </div>
            <div className="mw-form-row">
              <div className="label">Project root
                <span className="hint">Where MowisAI looks for repos. Each subfolder becomes a workspace.</span>
              </div>
              <div className="control">
                <input className="mw-input" defaultValue="/Users/you/code" />
              </div>
            </div>
            <div className="mw-form-row">
              <div className="label">Default fleet size
                <span className="hint">The planner caps parallel agents per session. You can override per-task.</span>
              </div>
              <div className="control">
                <input className="mw-input" defaultValue="32" style={{ maxWidth: 120 }} />
              </div>
            </div>
            <div className="mw-form-row">
              <div className="label">Sandbox provider
                <span className="hint">Where agents run. Local Docker, your cloud, or MowisAI cloud.</span>
              </div>
              <div className="control" style={{ flexDirection: 'row', gap: 0 }}>
                {['Docker (local)', 'Your cloud', 'MowisAI cloud'].map((opt, i) =>
                <button key={opt} className="mw-btn" style={{
                  borderRadius: 0,
                  borderRight: i < 2 ? 'none' : '1px solid var(--line-2)',
                  background: i === 2 ? 'rgba(47,134,255,0.08)' : 'transparent',
                  color: i === 2 ? 'var(--tx)' : 'var(--tx-2)',
                  fontStyle: i === 2 ? 'italic' : 'normal'
                }}>{opt}</button>
                )}
              </div>
            </div>
            <div className="mw-form-row">
              <div className="label">Theme
                <span className="hint">MowisAI is designed for dark.</span>
              </div>
              <div className="control" style={{ flexDirection: 'row', gap: 8 }}>
                <button className="mw-btn" style={{ background: 'rgba(47,134,255,0.08)' }}><span style={{ fontStyle: 'italic' }}>Dark</span></button>
                <button className="mw-btn ghost">Light (coming)</button>
              </div>
            </div>
            <div className="mw-form-row">
              <div className="label">Telemetry
                <span className="hint">Anonymous performance traces. No code, no prompts, no completions.</span>
              </div>
              <div className="control">
                <button className="mw-btn">Off</button>
              </div>
            </div>
          </div>
        </main>
      </div>
      <StatusBar agents={0} sandboxes={0} tokens="0" />
    </div>);

}

// Export everything to window for the entry script.
Object.assign(window, {
  WelcomeScreen,
  HomeScreen,
  SessionScreen,
  AgentDetailScreen,
  SpawnScreen,
  SandboxesScreen,
  TimelineScreen,
  SettingsScreen
});