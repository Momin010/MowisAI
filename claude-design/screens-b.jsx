/* global React */
// MowisAI — Variation B (Composed / Editorial Grid)
// Same screens, but a more structured, magazine-grid approach: serif numerals
// at large sizes, hairline rules everywhere, and the agents panel re-imagined
// as a vertical "register" rather than a tile grid.

const { useState: useStateB, useEffect: useEffectB } = React;

const AGENTS_B = [
  { id: 'A02', name: 'auth-1', task: 'Replacing legacy session middleware with rotating-token boundary.', sandbox: 'backend-α', status: 'running', step: 'II / V', for: '14m' },
  { id: 'A03', name: 'auth-2', task: 'Migrating user model for multi-factor enrolment.', sandbox: 'backend-β', status: 'running', step: 'I / IV', for: '08m' },
  { id: 'A04', name: 'auth-3', task: 'Rewriting password hash function to argon2id.', sandbox: 'backend-γ', status: 'running', step: 'IV / VI', for: '11m' },
  { id: 'A05', name: 'fe-login', task: 'Recomposing login form with new validation.', sandbox: 'frontend', status: 'running', step: 'II / III', for: '06m' },
  { id: 'A06', name: 'fe-2fa', task: 'Building OTP entry surface and fallback flows.', sandbox: 'frontend', status: 'running', step: 'I / IV', for: '02m' },
  { id: 'A07', name: 'tests-unit', task: 'Generating unit tests for new session boundary.', sandbox: 'ci-α', status: 'running', step: 'V / IX', for: '18m' },
  { id: 'A08', name: 'tests-int', task: 'Drafting integration tests across services.', sandbox: 'ci-β', status: 'running', step: 'III / VII', for: '12m' },
  { id: 'A09', name: 'docs', task: 'Updating API reference for new endpoints.', sandbox: 'docs', status: 'running', step: 'II / III', for: '04m' },
  { id: 'A10', name: 'mig-db', task: 'Writing reversible migration for users table.', sandbox: 'db', status: 'running', step: 'I / II', for: '01m' },
  { id: 'A11', name: 'review-1', task: 'Reading diff for security regressions.', sandbox: 'review', status: 'running', step: '—', for: '00m' },
  { id: 'A12', name: 'review-2', task: 'Cross-checking style and naming consistency.', sandbox: 'review', status: 'running', step: '—', for: '00m' },
  { id: 'A13', name: 'planner', task: 'Watching the fleet for redispatch opportunities.', sandbox: 'planner', status: 'running', step: '—', for: '14m' },
  { id: 'B01', name: 'pool-1', task: 'Idle. Waiting.', sandbox: 'pool', status: 'idle', step: '', for: '' },
  { id: 'B02', name: 'pool-2', task: 'Idle. Waiting.', sandbox: 'pool', status: 'idle', step: '', for: '' },
  { id: 'C01', name: 'lint', task: 'Done — formatting on backend module.', sandbox: 'ci-α', status: 'done', step: '✓', for: '' },
  { id: 'C02', name: 'audit', task: 'Done — license audit clean.', sandbox: 'review', status: 'done', step: '✓', for: '' },
];

function TitleBarB({ crumb = ['MowisAI', 'Session', 'feature/auth-refactor'] }) {
  return (
    <div className="mw-titlebar">
      <div className="mw-traffic"><span/><span/><span/></div>
      <div className="mw-titlebar-center">
        {crumb.map((c, i) => (
          <React.Fragment key={i}>
            {i > 0 && <span className="sep">·</span>}
            <span style={{ fontStyle: i === crumb.length - 1 ? 'italic' : 'normal' }}>{c}</span>
          </React.Fragment>
        ))}
      </div>
      <div className="mw-titlebar-right">
        <span><span className="mw-status-dot"/>FLEET LIVE</span>
        <span style={{ fontFamily: 'var(--mono)' }}>14:22:08</span>
      </div>
    </div>
  );
}

function StatusBarB() {
  return (
    <div className="mw-statusbar">
      <span><span className="mw-status-dot" style={{ marginRight: 8 }}/><b>DAEMON</b></span>
      <span className="sep">·</span>
      <span><b style={{ fontFamily: 'var(--serif)', fontSize: 12, fontStyle: 'italic' }}>twelve</b> agents</span>
      <span className="sep">·</span>
      <span><b style={{ fontFamily: 'var(--serif)', fontSize: 12, fontStyle: 'italic' }}>six</b> sandboxes</span>
      <span className="sep">·</span>
      <span><b>184k</b> tokens · hr</span>
      <span style={{ marginLeft: 'auto' }}>main · clean · ⌘K to act</span>
    </div>
  );
}

function SidebarB({ active = 'session' }) {
  const items = [
    { id: 'home', label: 'I.', name: 'Home' },
    { id: 'session', label: 'II.', name: 'Session' },
    { id: 'agents', label: 'III.', name: 'Fleet' },
    { id: 'sandboxes', label: 'IV.', name: 'Sandboxes' },
    { id: 'timeline', label: 'V.', name: 'Timeline' },
    { id: 'settings', label: 'VI.', name: 'Settings' },
  ];
  return (
    <aside className="mw-sidebar" style={{ width: 220, padding: '26px 18px 16px 22px' }}>
      <div className="mw-brand" style={{ fontSize: 20, marginBottom: 44 }}>
        <span>Mowis</span><span style={{ fontStyle: 'italic' }}>AI</span>
        <span className="dot"/>
      </div>
      <div className="mw-section-label" style={{ paddingLeft: 0 }}>Volumes</div>
      {items.map(it => (
        <div key={it.id} className={`mw-nav-item ${it.id === active ? 'active' : ''}`} style={{ padding: '8px 0', display: 'grid', gridTemplateColumns: '28px 1fr', alignItems: 'baseline' }}>
          <span style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.04em' }}>{it.label}</span>
          <span style={{ fontStyle: it.id === active ? 'italic' : 'normal' }}>{it.name}</span>
        </div>
      ))}
      <div className="mw-sidebar-spacer"/>
      <div style={{ borderTop: '1px solid var(--line)', paddingTop: 14, fontFamily: 'var(--serif)', fontSize: 12, color: 'var(--tx-3)', lineHeight: 1.5 }}>
        <div style={{ fontStyle: 'italic', color: 'var(--tx-2)' }}>you@mowis.ai</div>
        <div style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.10em', textTransform: 'uppercase', marginTop: 4 }}>Pro · 320h</div>
      </div>
    </aside>
  );
}

// Vertical agent register — like a magazine contents page
function AgentRegisterB({ side = 'right', density = 'sparse', count = 12 }) {
  const [hovered, setHovered] = useStateB(null);
  const running = AGENTS_B.filter(a => a.status === 'running').slice(0, count);
  const idle = AGENTS_B.filter(a => a.status === 'idle');
  const done = AGENTS_B.filter(a => a.status === 'done');
  const all = [...running, ...idle, ...done];
  const cap = density === 'dense' ? 16 : density === 'balanced' ? 12 : 9;
  const list = all.slice(0, cap);
  return (
    <aside className="mw-agents" style={{
      width: 300,
      ...(side === 'left' ? { borderLeft: 'none', borderRight: '1px solid var(--line)', order: -1 } : {})
    }}>
      <div className="mw-agents-head">
        <div className="title">
          <span style={{ fontStyle: 'italic' }}>The fleet</span>
          <span style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-3)' }}>{running.length}/{AGENTS_B.length}</span>
        </div>
        <div className="meta">Register · live</div>
      </div>
      <div style={{ overflowY: 'auto', flex: 1 }}>
        {list.map((a, i) => (
          <div
            key={a.id + i}
            onMouseEnter={() => setHovered(i)}
            onMouseLeave={() => setHovered(null)}
            style={{
              padding: '14px 18px',
              borderBottom: '1px solid var(--line)',
              display: 'grid',
              gridTemplateColumns: '40px 1fr auto',
              gap: 14,
              alignItems: 'baseline',
              cursor: 'pointer',
              background: hovered === i ? 'rgba(255,255,255,0.025)' : 'transparent',
              position: 'relative',
            }}
          >
            <div style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.04em' }}>{a.id}</div>
            <div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 14, color: 'var(--tx)', fontStyle: 'italic', marginBottom: 3 }}>
                {a.name}
              </div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 12, color: 'var(--tx-3)', lineHeight: 1.4 }}>
                {a.task}
              </div>
              {hovered === i && a.status === 'running' && (
                <div style={{ fontFamily: 'var(--sans)', fontSize: 9, color: 'var(--tx-4)', letterSpacing: '0.14em', textTransform: 'uppercase', marginTop: 8, display: 'flex', gap: 14 }}>
                  <span><b style={{ color: 'var(--tx-2)', fontWeight: 400 }}>{a.sandbox}</b></span>
                  <span><b style={{ color: 'var(--tx-2)', fontWeight: 400 }}>{a.step}</b></span>
                  <span><b style={{ color: 'var(--tx-2)', fontWeight: 400 }}>{a.for}</b></span>
                </div>
              )}
            </div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'flex-end' }}>
              {a.status === 'running' ? (
                <span style={{
                  width: 6, height: 6, borderRadius: '50%',
                  background: 'var(--blue)',
                  boxShadow: '0 0 8px var(--blue-glow)',
                  animation: 'pulse 1.8s ease-in-out infinite',
                }}/>
              ) : a.status === 'idle' ? (
                <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'rgba(255,255,255,0.20)' }}/>
              ) : (
                <span style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-4)' }}>✓</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </aside>
  );
}

// ─── Welcome (sharper, with sub-line) ───────────────────────
function WelcomeScreenB({ blur = 70 }) {
  const [b, setB] = useStateB(blur * 0.2);
  useEffectB(() => {
    const t = setTimeout(() => setB(blur * 0.02), 200);
    return () => clearTimeout(t);
  }, [blur]);
  return (
    <div style={{ width: '100%', height: '100%', background: '#000', display: 'flex', flexDirection: 'column', borderRadius: 12, overflow: 'hidden', position: 'relative' }}>
      <div className="mw-titlebar" style={{ background: 'transparent', borderBottom: 'none' }}>
        <div className="mw-traffic"><span/><span/><span/></div>
      </div>
      <div className="mw-welcome" style={{ flexDirection: 'column', gap: 18 }}>
        <div className="word" style={{ filter: `blur(${b}px)`, transition: 'filter 3.2s cubic-bezier(.2,.7,.2,1)', fontSize: 80 }}>
          Welcome to MowisAI
        </div>
        <div style={{
          fontFamily: 'var(--serif)',
          fontSize: 14,
          fontStyle: 'italic',
          color: 'var(--tx-3)',
          letterSpacing: '0.02em',
          filter: `blur(${b * 0.35}px)`,
          transition: 'filter 3.2s cubic-bezier(.2,.7,.2,1)',
        }}>
          A studio for thousands of agents.
        </div>
        <div style={{
          position: 'absolute', top: 56, left: 56,
          fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-4)',
          letterSpacing: '0.18em', textTransform: 'uppercase',
        }}>
          Vol. I — Threshold
        </div>
        <div className="continue">
          <button>continue —</button>
        </div>
      </div>
    </div>
  );
}

// ─── Home (composed) ────────────────────────────────────────
function HomeScreenB() {
  const recent = [
    { num: '01', name: 'feature/auth-refactor', time: 'today, 14:08', repo: 'mowis/api', agents: 12, state: 'running' },
    { num: '02', name: 'fix/payments-webhook', time: 'yesterday', repo: 'mowis/api', agents: 6, state: 'merged' },
    { num: '03', name: 'refactor/onboarding-flow', time: 'Mon, Apr 22', repo: 'mowis/web', agents: 18, state: 'merged' },
    { num: '04', name: 'spike/embedding-eval', time: 'Apr 20', repo: 'mowis/research', agents: 4, state: 'archived' },
  ];
  return (
    <div className="mw-app">
      <TitleBarB crumb={['MowisAI', 'Home']}/>
      <div className="mw-body">
        <SidebarB active="home"/>
        <main className="mw-main">
          <div className="mw-page-head" style={{ padding: '40px 56px 28px', alignItems: 'flex-start' }}>
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 8px' }}>Vol. II — Studio</div>
              <h1 style={{ fontSize: 40, lineHeight: 1.05 }}>
                Begin a session,<br/>
                <span style={{ fontStyle: 'italic', color: 'var(--tx-2)' }}>or return to one.</span>
              </h1>
            </div>
            <button className="mw-btn primary" style={{ padding: '10px 20px' }}>+ New session</button>
          </div>
          <div className="mw-page-body" style={{ padding: '0 56px 40px' }}>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 0, borderTop: '1px solid var(--line)' }}>
              <div style={{ padding: '32px 32px 32px 0', borderRight: '1px solid var(--line)' }}>
                <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>Open session</div>
                <div style={{ fontFamily: 'var(--serif)', fontSize: 24, color: 'var(--tx)', margin: '8px 0 14px', fontStyle: 'italic' }}>
                  feature/auth-refactor
                </div>
                <div style={{ fontFamily: 'var(--serif)', fontSize: 14, color: 'var(--tx-2)', lineHeight: 1.6 }}>
                  Twelve agents are presently composing a refactor of the authentication layer across four sandboxes. One has stopped to ask a question about token policy.
                </div>
                <div style={{ display: 'flex', gap: 10, marginTop: 18 }}>
                  <button className="mw-btn primary">Resume</button>
                  <button className="mw-btn ghost">Pause fleet</button>
                </div>
              </div>
              <div style={{ padding: '32px 0 32px 32px' }}>
                <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>This week</div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 0, marginTop: 14 }}>
                  <div className="mw-stat" style={{ padding: '0 0 18px', borderBottom: '1px solid var(--line)' }}>
                    <div className="value">47<sub>sessions</sub></div>
                    <div className="label">Begun</div>
                  </div>
                  <div className="mw-stat" style={{ padding: '0 0 18px', borderBottom: '1px solid var(--line)', paddingLeft: 24 }}>
                    <div className="value">312<sub>agents</sub></div>
                    <div className="label">Dispatched</div>
                  </div>
                  <div className="mw-stat" style={{ padding: '18px 0 0', borderBottom: 'none' }}>
                    <div className="value">94<sub>%</sub></div>
                    <div className="label">Approved</div>
                  </div>
                  <div className="mw-stat" style={{ padding: '18px 0 0', borderBottom: 'none', paddingLeft: 24 }}>
                    <div className="value">$184<sub>spent</sub></div>
                    <div className="label">This week</div>
                  </div>
                </div>
              </div>
            </div>
            <div className="mw-section-label" style={{ padding: 0, margin: '36px 0 0' }}>Past sessions</div>
            <table className="mw-table" style={{ marginTop: 8 }}>
              <thead>
                <tr><th style={{ width: 32 }}>№</th><th>Branch</th><th>Repository</th><th>Agents</th><th>State</th><th style={{ textAlign: 'right' }}>When</th></tr>
              </thead>
              <tbody>
                {recent.map(r => (
                  <tr key={r.num}>
                    <td className="mono">{r.num}</td>
                    <td className="tx" style={{ fontStyle: 'italic' }}>{r.name}</td>
                    <td className="mono">{r.repo}</td>
                    <td>{r.agents}</td>
                    <td><span className={`pill ${r.state === 'running' ? 'running' : r.state === 'archived' ? 'queued' : 'done'}`}>{r.state}</span></td>
                    <td style={{ textAlign: 'right' }}>{r.time}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </main>
      </div>
      <StatusBarB/>
    </div>
  );
}

// ─── Session (composed) ─────────────────────────────────────
function SessionScreenB({ panelSide = 'right', density = 'sparse', agentCount = 12 }) {
  return (
    <div className="mw-app">
      <TitleBarB/>
      <div className="mw-body">
        <SidebarB active="session"/>
        <main className="mw-main">
          {/* header strip */}
          <div style={{ padding: '22px 56px 18px', borderBottom: '1px solid var(--line)', display: 'grid', gridTemplateColumns: '1fr auto auto', gap: 32, alignItems: 'baseline' }}>
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Vol. II · Session i</div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 22, color: 'var(--tx)' }}>
                <span style={{ fontStyle: 'italic' }}>The authentication refactor</span>
              </div>
            </div>
            <div style={{ textAlign: 'right' }}>
              <div className="mw-section-label" style={{ padding: 0, margin: 0, textAlign: 'right' }}>Elapsed</div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 22, color: 'var(--tx)', fontVariantNumeric: 'tabular-nums' }}>14:22</div>
            </div>
            <div style={{ textAlign: 'right' }}>
              <div className="mw-section-label" style={{ padding: 0, margin: 0, textAlign: 'right' }}>Progress</div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 22, color: 'var(--tx)', fontStyle: 'italic' }}>3 / 8</div>
            </div>
          </div>
          <div className="mw-chat" style={{ padding: '28px 56px' }}>
            <div className="turn">
              <div className="role">You · 14:08</div>
              <div className="body">
                Refactor the authentication layer to support multi-factor and rotating tokens. Update the docs and tests as you go.
              </div>
            </div>
            <div className="system">
              <span className="tag">i</span>
              <span>Planner produced eight tasks · twelve agents dispatched · four sandboxes opened</span>
            </div>
            <div className="turn">
              <div className="role">Planner · 14:09</div>
              <div className="body muted">
                Eight tasks. Three independent and parallel. Two depend on the migration. Review pair set on every branch. I will surface only what drifts from the existing voice.
              </div>
            </div>
            <div className="system">
              <span className="tag">ii</span>
              <span>auth-1 has stopped — refresh TTL exceeds policy in SECURITY.md</span>
            </div>
            <div className="turn">
              <div className="role">auth-1 · 14:14 <span style={{ color: 'var(--blue)', marginLeft: 12, fontStyle: 'italic' }}>awaiting input</span></div>
              <div className="body">
                The refresh token TTL I picked is 30 days. <span style={{ color: 'var(--tx-3)' }}>SECURITY.md</span> caps refresh at 14 days. Should I tighten to 14, or amend the policy first?
              </div>
              <div style={{ display: 'flex', gap: 10, marginTop: 12 }}>
                <button className="mw-btn">Tighten to 14d</button>
                <button className="mw-btn">Amend policy</button>
                <button className="mw-btn ghost">Reply…</button>
              </div>
            </div>
          </div>
          <div className="mw-composer" style={{ padding: '14px 56px 18px' }}>
            <textarea className="input" placeholder="Reply, redirect, or @mention an agent…"/>
            <div className="row">
              <span className="chip">@auth-1</span>
              <span className="chip">file: SECURITY.md</span>
              <span style={{ flex: 1 }}/>
              <button className="mw-btn ghost">Pause all</button>
              <button className="mw-btn primary send">Send</button>
            </div>
          </div>
        </main>
        <AgentRegisterB side={panelSide} density={density} count={agentCount}/>
      </div>
      <StatusBarB/>
    </div>
  );
}

// ─── Agent detail (composed) ────────────────────────────────
function AgentDetailScreenB() {
  const log = [
    { t: '14:08:42', text: 'Spawned · backend-α · from planner.' },
    { t: '14:09:31', text: 'Drafted token boundary contract.' },
    { t: '14:11:14', text: 'Wrote api/auth/token_boundary.py.' },
    { t: '14:12:08', text: 'Replaced 14 references to SessionMiddleware across 6 files.' },
    { t: '14:13:55', text: 'Ran the auth unit suite. 47 passed; 2 failing in oauth.' },
    { t: '14:14:18', text: 'Asked review-1 for a security read on the refresh TTL.' },
    { t: '14:14:31', text: 'Stopped — awaiting human input on TTL.' },
  ];
  return (
    <div className="mw-app">
      <TitleBarB crumb={['MowisAI', 'Session', 'fleet', 'auth-1']}/>
      <div className="mw-body">
        <SidebarB active="agents"/>
        <main className="mw-main">
          <div style={{ padding: '40px 56px 28px', borderBottom: '1px solid var(--line)' }}>
            <div style={{ display: 'flex', alignItems: 'baseline', gap: 22, marginBottom: 12 }}>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 80, lineHeight: 1, color: 'var(--tx)', letterSpacing: '-0.02em' }}>A02</div>
              <div style={{ flex: 1 }}>
                <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Agent · running 14m 22s</div>
                <h1 style={{ fontSize: 32, margin: 0, fontStyle: 'italic' }}>auth-1</h1>
                <div style={{ fontFamily: 'var(--serif)', fontSize: 15, color: 'var(--tx-2)', marginTop: 6 }}>
                  Replacing the legacy session middleware with a rotating-token boundary that binds refresh to device fingerprint.
                </div>
              </div>
              <div style={{ display: 'flex', gap: 10 }}>
                <button className="mw-btn ghost">Pause</button>
                <button className="mw-btn ghost">Fork</button>
                <button className="mw-btn">Open sandbox</button>
              </div>
            </div>
          </div>
          <div className="mw-detail" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <section>
              <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>The block</div>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 17, color: 'var(--tx-2)', lineHeight: 1.55, margin: '12px 0 0', fontStyle: 'italic', borderLeft: '1px solid var(--line-2)', paddingLeft: 18 }}>
                "The refresh token TTL I picked (30 days) exceeds the policy in <span style={{ fontFamily: 'var(--mono)', fontStyle: 'normal', fontSize: 14 }}>SECURITY.md</span> (14 days). Should I tighten to 14 days, or amend the policy first?"
              </div>
              <div style={{ display: 'flex', gap: 10, marginTop: 18 }}>
                <button className="mw-btn">Tighten to 14d</button>
                <button className="mw-btn">Amend policy</button>
                <button className="mw-btn ghost">Reply…</button>
              </div>
              <div className="mw-section-label" style={{ padding: 0, margin: '32px 0 0' }}>Particulars</div>
              <dl className="mw-kv" style={{ marginTop: 10 }}>
                <dt>Sandbox</dt><dd className="mono">backend-α · py-3.12</dd>
                <dt>Branch</dt><dd className="mono">refactor/auth/token-boundary</dd>
                <dt>Spawned</dt><dd>14:08:42 · by planner</dd>
                <dt>Cost</dt><dd>$1.84 · 92k tokens</dd>
                <dt>Files</dt><dd className="mono">7 changed · +218 / −94</dd>
                <dt>Step</dt><dd style={{ fontStyle: 'italic' }}>II of V — Review</dd>
              </dl>
            </section>
            <section>
              <div className="mw-section-label" style={{ padding: 0, margin: 0 }}>Activity</div>
              <div className="mw-timeline" style={{ marginTop: 10 }}>
                {log.map((l, i) => (
                  <div className="row" key={i} style={{ gridTemplateColumns: '70px 1fr' }}>
                    <div className="time">{l.t}</div>
                    <div className="text">{l.text}</div>
                  </div>
                ))}
              </div>
            </section>
          </div>
        </main>
      </div>
      <StatusBarB/>
    </div>
  );
}

// ─── Spawn (composed) ────────────────────────────────────────
function SpawnScreenB() {
  return (
    <div className="mw-app">
      <TitleBarB crumb={['MowisAI', 'New session']}/>
      <div className="mw-body">
        <SidebarB active="home"/>
        <main className="mw-main">
          <div style={{ flex: 1, display: 'grid', gridTemplateRows: '1fr auto', overflow: 'hidden' }}>
            <div style={{ overflow: 'auto', padding: '64px 56px 32px' }}>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 14px' }}>Vol. II · A new session</div>
              <h1 style={{ fontFamily: 'var(--serif)', fontSize: 56, margin: 0, lineHeight: 1.05, letterSpacing: '-0.015em', color: 'var(--tx)' }}>
                State the goal,<br/>
                <span style={{ fontStyle: 'italic', color: 'var(--tx-2)' }}>plainly.</span>
              </h1>
              <div style={{ fontFamily: 'var(--serif)', fontSize: 16, color: 'var(--tx-3)', maxWidth: 580, lineHeight: 1.6, marginTop: 18, fontStyle: 'italic' }}>
                The planner reads it, decomposes it, and dispatches a fleet. You stay in the loop only where it matters.
              </div>
              <div style={{ marginTop: 38, display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 0, borderTop: '1px solid var(--line)', borderBottom: '1px solid var(--line)' }}>
                {[
                  { num: 'I', label: 'Repository', value: 'mowis/api' },
                  { num: 'II', label: 'Branch', value: 'main → refactor/*' },
                  { num: 'III', label: 'Fleet size', value: 'auto · up to 32' },
                ].map(c => (
                  <div key={c.num} style={{ padding: '20px 24px', borderRight: c.num !== 'III' ? '1px solid var(--line)' : 'none' }}>
                    <div style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.10em' }}>{c.num}.</div>
                    <div style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.16em', textTransform: 'uppercase', margin: '6px 0' }}>{c.label}</div>
                    <div style={{ fontFamily: 'var(--serif)', fontSize: 16, color: 'var(--tx)', fontStyle: 'italic' }}>{c.value}</div>
                  </div>
                ))}
              </div>
              <div className="mw-section-label" style={{ padding: 0, margin: '36px 0 14px' }}>From the studio</div>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 0, borderTop: '1px solid var(--line)' }}>
                {[
                  ['Audit the codebase for n+1 queries', '8 agents · 12m'],
                  ['Migrate the worker queue to Redis Streams', '14 agents · 28m'],
                  ['Generate integration tests for billing', '6 agents · 18m'],
                  ['Translate marketing site to FR & DE', '10 agents · 22m'],
                ].map(([s, m], i) => (
                  <div key={i} style={{ padding: '16px 0', borderBottom: '1px solid var(--line)', borderRight: i % 2 === 0 ? '1px solid var(--line)' : 'none', paddingLeft: i % 2 === 1 ? 24 : 0, paddingRight: i % 2 === 0 ? 24 : 0, display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', cursor: 'pointer' }}>
                    <div style={{ fontFamily: 'var(--serif)', fontSize: 15, color: 'var(--tx)', fontStyle: 'italic' }}>{s}</div>
                    <div style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.10em', textTransform: 'uppercase' }}>{m}</div>
                  </div>
                ))}
              </div>
            </div>
            <div style={{ padding: '18px 56px 22px', borderTop: '1px solid var(--line)', background: 'rgba(0,0,0,0.5)' }}>
              <div className="mw-spawn-input" style={{ maxWidth: 'none', padding: '18px 22px' }}>
                <textarea placeholder="Refactor the authentication layer to support multi-factor and rotating tokens." style={{ height: 50 }}/>
                <div className="controls">
                  <span style={{ fontFamily: 'var(--mono)', fontSize: 10, color: 'var(--tx-3)', letterSpacing: '0.06em' }}>⌘ + ↵ to dispatch</span>
                  <span style={{ flex: 1 }}/>
                  <button className="mw-btn ghost">Plan only</button>
                  <button className="mw-btn primary">Dispatch fleet</button>
                </div>
              </div>
            </div>
          </div>
        </main>
      </div>
      <StatusBarB/>
    </div>
  );
}

// ─── Sandboxes (composed) ───────────────────────────────────
function SandboxesScreenB() {
  const sb = [
    { num: 'I', name: 'backend-α', tag: 'py-3.12 · 4 vCPU · 8 GB', agents: ['auth-1', 'auth-2', 'auth-3', 'mig-db'], cpu: 41, ram: 62 },
    { num: 'II', name: 'backend-β', tag: 'py-3.12 · 2 vCPU · 4 GB', agents: ['tests-int', 'docs'], cpu: 22, ram: 34 },
    { num: 'III', name: 'frontend', tag: 'node-20 · 4 vCPU · 8 GB', agents: ['fe-login', 'fe-2fa', 'fe-style'], cpu: 55, ram: 48 },
    { num: 'IV', name: 'db', tag: 'pg-16 · 2 vCPU · 4 GB', agents: ['mig-db'], cpu: 11, ram: 18 },
    { num: 'V', name: 'ci-α', tag: 'ubuntu-22 · 8 vCPU · 16 GB', agents: ['tests-unit'], cpu: 30, ram: 21 },
    { num: 'VI', name: 'review', tag: 'observer · 1 vCPU · 2 GB', agents: ['review-1', 'review-2'], cpu: 4, ram: 10 },
  ];
  return (
    <div className="mw-app">
      <TitleBarB crumb={['MowisAI', 'Session', 'sandboxes']}/>
      <div className="mw-body">
        <SidebarB active="sandboxes"/>
        <main className="mw-main">
          <div className="mw-page-head" style={{ padding: '40px 56px 28px' }}>
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Vol. IV · Environments</div>
              <h1 style={{ fontSize: 36 }}><span style={{ fontStyle: 'italic' }}>Sandboxes</span></h1>
              <div className="sub">six running · thirteen agents attached</div>
            </div>
            <div style={{ display: 'flex', gap: 10 }}>
              <button className="mw-btn ghost">Stop idle</button>
              <button className="mw-btn">+ Sandbox</button>
            </div>
          </div>
          <div className="mw-page-body" style={{ padding: '0 56px 40px' }}>
            <table className="mw-table">
              <thead>
                <tr>
                  <th style={{ width: 28 }}>№</th>
                  <th>Name</th>
                  <th>Image</th>
                  <th>Agents</th>
                  <th style={{ width: 180 }}>CPU</th>
                  <th style={{ width: 180 }}>Memory</th>
                </tr>
              </thead>
              <tbody>
                {sb.map(s => (
                  <tr key={s.name}>
                    <td className="mono">{s.num}</td>
                    <td className="tx" style={{ fontStyle: 'italic', fontSize: 15 }}>{s.name}</td>
                    <td className="mono">{s.tag}</td>
                    <td>
                      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                        {s.agents.map(a => (
                          <span key={a} className="pill" style={{ fontFamily: 'var(--mono)', textTransform: 'none', letterSpacing: '0.02em' }}>{a}</span>
                        ))}
                      </div>
                    </td>
                    <td>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                        <div style={{ flex: 1, height: 2, background: 'rgba(255,255,255,0.08)', borderRadius: 2 }}>
                          <div style={{ width: `${s.cpu}%`, height: '100%', background: 'var(--blue)', borderRadius: 2 }}/>
                        </div>
                        <span className="mono" style={{ fontSize: 11, minWidth: 36, textAlign: 'right' }}>{s.cpu}%</span>
                      </div>
                    </td>
                    <td>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                        <div style={{ flex: 1, height: 2, background: 'rgba(255,255,255,0.08)', borderRadius: 2 }}>
                          <div style={{ width: `${s.ram}%`, height: '100%', background: 'rgba(255,255,255,0.45)', borderRadius: 2 }}/>
                        </div>
                        <span className="mono" style={{ fontSize: 11, minWidth: 36, textAlign: 'right' }}>{s.ram}%</span>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </main>
      </div>
      <StatusBarB/>
    </div>
  );
}

// ─── Timeline (composed) ────────────────────────────────────
function TimelineScreenB() {
  const events = [
    { t: '14:14:31', who: 'auth-1', text: 'asked for input on refresh token TTL.', tag: 'block' },
    { t: '14:14:18', who: 'review-1', text: 'flagged a security policy mismatch on auth-1.', tag: 'review' },
    { t: '14:13:55', who: 'auth-1', text: 'ran the auth unit suite — 47 passed, 2 failing.', tag: 'tests' },
    { t: '14:12:08', who: 'auth-1', text: 'replaced 14 references to SessionMiddleware across six files.', tag: 'edit' },
    { t: '14:11:42', who: 'fe-login', text: 'rendered new validation states for the login form.', tag: 'edit' },
    { t: '14:11:14', who: 'auth-1', text: 'wrote api/auth/token_boundary.py.', tag: 'edit' },
    { t: '14:10:33', who: 'mig-db', text: 'wrote a reversible migration for the users table.', tag: 'db' },
    { t: '14:09:21', who: 'planner', text: 'dispatched twelve agents across four sandboxes.', tag: 'plan' },
    { t: '14:09:02', who: 'planner', text: 'produced eight tasks from your goal.', tag: 'plan' },
    { t: '14:08:42', who: 'you', text: 'started a session: refactor the authentication layer.', tag: 'start' },
  ];
  return (
    <div className="mw-app">
      <TitleBarB crumb={['MowisAI', 'Session', 'timeline']}/>
      <div className="mw-body">
        <SidebarB active="timeline"/>
        <main className="mw-main">
          <div className="mw-page-head" style={{ padding: '40px 56px 28px' }}>
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Vol. V · Record</div>
              <h1 style={{ fontSize: 36 }}><span style={{ fontStyle: 'italic' }}>Timeline</span></h1>
              <div className="sub">since 14:08 · live · ten events</div>
            </div>
            <div style={{ display: 'flex', gap: 10 }}>
              <button className="mw-btn ghost">All agents</button>
              <button className="mw-btn ghost">Filter</button>
              <button className="mw-btn ghost">Export</button>
            </div>
          </div>
          <div className="mw-page-body" style={{ padding: '0 56px 40px' }}>
            <div style={{ position: 'relative', paddingLeft: 18 }}>
              <div style={{ position: 'absolute', top: 6, bottom: 6, left: 4, width: 1, background: 'var(--line)' }}/>
              {events.map((e, i) => (
                <div key={i} style={{ display: 'grid', gridTemplateColumns: '90px 1fr 90px', gap: 24, padding: '16px 0', borderBottom: '1px solid var(--line)', position: 'relative' }}>
                  <div style={{ position: 'absolute', left: -18, top: 22, width: 9, height: 9, borderRadius: '50%', background: e.tag === 'block' ? 'var(--blue)' : 'rgba(255,255,255,0.3)', boxShadow: e.tag === 'block' ? '0 0 8px var(--blue-glow)' : 'none' }}/>
                  <div className="time" style={{ fontFamily: 'var(--mono)', fontSize: 11, color: 'var(--tx-4)' }}>{e.t}</div>
                  <div style={{ fontFamily: 'var(--serif)', fontSize: 15, color: 'var(--tx-2)', lineHeight: 1.45 }}>
                    <span style={{ color: 'var(--tx)', fontStyle: 'italic' }}>{e.who}</span> {e.text}
                  </div>
                  <div style={{ fontFamily: 'var(--sans)', fontSize: 10, color: 'var(--tx-4)', letterSpacing: '0.14em', textTransform: 'uppercase', textAlign: 'right' }}>{e.tag}</div>
                </div>
              ))}
            </div>
          </div>
        </main>
      </div>
      <StatusBarB/>
    </div>
  );
}

// ─── Settings (composed) ────────────────────────────────────
function SettingsScreenB() {
  const groups = [
    { num: 'I', name: 'Account', rows: [
      { label: 'API key', hint: 'Used by the local daemon. Never leaves this machine.', value: 'mw_live_••••••••2f7c', actions: ['Reveal', 'Rotate'] },
      { label: 'Email', hint: 'For session digests and policy notes.', value: 'you@mowis.ai' },
    ]},
    { num: 'II', name: 'Studio', rows: [
      { label: 'Project root', hint: 'Each subfolder becomes a workspace.', value: '/Users/you/code' },
      { label: 'Default fleet', hint: 'Cap on parallel agents per session.', value: '32' },
    ]},
    { num: 'III', name: 'Compute', rows: [
      { label: 'Sandbox provider', hint: 'Where agents physically run.', value: 'MowisAI cloud', actions: ['Local', 'Your cloud', 'MowisAI cloud'] },
      { label: 'Region', hint: 'Affects latency and data residency.', value: 'us-west-2' },
    ]},
  ];
  return (
    <div className="mw-app">
      <TitleBarB crumb={['MowisAI', 'Settings']}/>
      <div className="mw-body">
        <SidebarB active="settings"/>
        <main className="mw-main">
          <div className="mw-page-head" style={{ padding: '40px 56px 28px' }}>
            <div>
              <div className="mw-section-label" style={{ padding: 0, margin: '0 0 4px' }}>Vol. VI · Preferences</div>
              <h1 style={{ fontSize: 36 }}><span style={{ fontStyle: 'italic' }}>Settings</span></h1>
              <div className="sub">workspace · you@mowis.ai</div>
            </div>
          </div>
          <div className="mw-page-body" style={{ padding: '0 56px 40px', maxWidth: 'none' }}>
            {groups.map(g => (
              <div key={g.num} style={{ marginTop: 22 }}>
                <div style={{ display: 'flex', alignItems: 'baseline', gap: 14, padding: '0 0 12px', borderBottom: '1px solid var(--line)' }}>
                  <span style={{ fontFamily: 'var(--mono)', fontSize: 11, color: 'var(--tx-4)', letterSpacing: '0.10em' }}>{g.num}.</span>
                  <span style={{ fontFamily: 'var(--serif)', fontSize: 18, color: 'var(--tx)', fontStyle: 'italic' }}>{g.name}</span>
                </div>
                {g.rows.map(r => (
                  <div key={r.label} className="mw-form-row" style={{ gridTemplateColumns: '260px 1fr', maxWidth: 920 }}>
                    <div className="label">{r.label}<span className="hint">{r.hint}</span></div>
                    <div className="control">
                      {r.actions ? (
                        <div style={{ display: 'flex', gap: 0 }}>
                          {r.actions.map((a, i) => (
                            <button key={a} className="mw-btn" style={{
                              borderRadius: 0,
                              borderRight: i < r.actions.length - 1 ? 'none' : '1px solid var(--line-2)',
                              background: a === r.value ? 'rgba(47,134,255,0.08)' : 'transparent',
                              fontStyle: a === r.value ? 'italic' : 'normal',
                              color: a === r.value ? 'var(--tx)' : 'var(--tx-2)',
                            }}>{a}</button>
                          ))}
                        </div>
                      ) : (
                        <input className="mw-input" defaultValue={r.value}/>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            ))}
          </div>
        </main>
      </div>
      <StatusBarB/>
    </div>
  );
}

Object.assign(window, {
  WelcomeScreenB,
  HomeScreenB,
  SessionScreenB,
  AgentDetailScreenB,
  SpawnScreenB,
  SandboxesScreenB,
  TimelineScreenB,
  SettingsScreenB,
});
