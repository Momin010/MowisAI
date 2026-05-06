/**
 * MowisAI Desktop — Usage Page
 */

import { State, $, setText, escHtml } from './state.js';
import { invoke } from './bridge.js';
import { fmtNumber, fmtTs } from './utils.js';

// ── Helpers ──────────────────────────────────────────────────────────────────

export function fmtTokens(n) {
  if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(1) + 'B';
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
  return String(n);
}

export function fmtDurationSecs(s) {
  if (!s || s <= 0) return '0m';
  if (s < 60) return s + 's';
  if (s < 3600) return Math.floor(s / 60) + 'm ' + (s % 60 > 0 ? (s % 60) + 's' : '');
  return Math.floor(s / 3600) + 'h ' + Math.floor((s % 3600) / 60) + 'm';
}

export function smoothPath(points) {
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

// ── Page render ──────────────────────────────────────────────────────────────

export async function renderUsagePage() {
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

// ── Charts ───────────────────────────────────────────────────────────────────

export function renderUsageChart(usage, stats) {
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

  const yTicks = 4;
  let gridLines = '';
  for (let i = 0; i <= yTicks; i++) {
    const y = 20 + ((H - pad - 20) / yTicks) * i;
    const val = maxVal * (1 - i / yTicks);
    gridLines += `<line x1="${pad}" y1="${y}" x2="${W - padR}" y2="${y}" stroke="rgba(255,255,255,0.04)" stroke-width="1"/>`;
    gridLines += `<text x="${pad - 6}" y="${y + 3}" text-anchor="end" fill="rgba(255,255,255,0.2)" font-size="9" font-family="var(--mono)">${fmtTokens(Math.round(val))}</text>`;
  }

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
        <stop offset="0%" stop-color="rgba(255,255,255,0.08)"/>
        <stop offset="100%" stop-color="rgba(255,255,255,0)"/>
      </linearGradient>
    </defs>
    ${gridLines}
    <path d="${areaD}" fill="url(#chart-fill)" class="chart-area"/>
    <path d="${lineD}" fill="none" stroke="var(--tx-3)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" class="chart-line"/>
    ${pts.map((p, i) => `<circle cx="${p.x}" cy="${p.y}" r="4" fill="var(--tx)" opacity="0" class="chart-dot" data-idx="${i}" style="cursor:pointer"/>`).join('')}
    ${xLabels}
  </svg>
  <div class="chart-tooltip hidden" id="chart-tooltip"></div>`;

  const tooltip = el.querySelector('#chart-tooltip');
  const dots = el.querySelectorAll('.chart-dot');
  dots.forEach(dot => {
    dot.addEventListener('mouseenter', (e) => {
      const idx = parseInt(dot.dataset.idx);
      const pt = pts[idx];
      if (!pt || !tooltip) return;
      const item = pt.item;
      const tokens = fmtTokens(item.tokens || 0);
      const prompt = (item.prompt_short || item.prompt || 'Session').slice(0, 40);
      tooltip.innerHTML = `<div class="chart-tooltip-title">${escHtml(prompt)}</div><div class="chart-tooltip-value">${tokens} tokens</div>`;
      tooltip.classList.remove('hidden');
      const rect = el.getBoundingClientRect();
      const dotRect = dot.getBoundingClientRect();
      tooltip.style.left = (dotRect.left - rect.left + dotRect.width / 2) + 'px';
      tooltip.style.top = (dotRect.top - rect.top - 8) + 'px';
      dot.setAttribute('opacity', '1');
    });
    dot.addEventListener('mouseleave', () => {
      if (tooltip) tooltip.classList.add('hidden');
      dot.setAttribute('opacity', '0');
    });
  });

  const area = el.querySelector('.chart-area');
  const line = el.querySelector('.chart-line');
  if (area) area.style.animation = 'chartFadeIn 0.8s ease forwards';
  if (line) {
    const length = line.getTotalLength ? line.getTotalLength() : 1000;
    line.style.strokeDasharray = length;
    line.style.strokeDashoffset = length;
    line.style.animation = 'chartLineIn 1.2s ease forwards';
  }
}

export function renderDonutChart(stats) {
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
    { label: 'Code Generation', pct: 0.32, color: 'rgba(255,255,255,0.5)' },
    { label: 'Planning', pct: 0.22, color: 'rgba(255,255,255,0.35)' },
    { label: 'Tool Calls', pct: 0.20, color: 'rgba(255,255,255,0.25)' },
    { label: 'Code Review', pct: 0.15, color: 'rgba(255,255,255,0.15)' },
    { label: 'Thinking', pct: 0.11, color: 'rgba(255,255,255,0.08)' },
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

export function renderSuccessRate(hist) {
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
        <circle cx="60" cy="60" r="48" fill="none" stroke="var(--tx-3)" stroke-width="10" stroke-dasharray="${(pct / 100) * 301.6} ${301.6}" stroke-linecap="round" transform="rotate(-90 60 60)"/>
        <text x="60" y="56" text-anchor="middle" fill="var(--tx)" font-family="var(--serif)" font-size="24">${pct}%</text>
        <text x="60" y="72" text-anchor="middle" fill="var(--tx-4)" font-size="9" font-family="var(--sans)">success</text>
      </svg>
    </div>
    <div class="usage-rate-bars">
      <div class="usage-rate-bar-row"><span class="usage-rate-dot" style="background:var(--tx-3)"></span><span>Completed</span><span class="usage-rate-count">${done}</span></div>
      <div class="usage-rate-bar-row"><span class="usage-rate-dot" style="background:var(--tx-4)"></span><span>Failed</span><span class="usage-rate-count">${failed}</span></div>
      <div class="usage-rate-bar-row"><span class="usage-rate-dot" style="background:var(--tx-5)"></span><span>Stopped</span><span class="usage-rate-count">${stopped}</span></div>
    </div>`;
}

export function renderTimeline(hist) {
  const el = $('usage-timeline');
  if (!el) return;
  if (!hist.length) { el.innerHTML = '<div class="usage-rate-empty">No sessions</div>'; return; }

  const sorted = [...hist].sort((a, b) => a.started_at - b.started_at);
  const maxTokens = Math.max(...sorted.map(s => s.tokens || 0), 1);

  el.innerHTML = `<div class="usage-timeline-track">${sorted.map((s, i) => {
    const h = Math.max(8, ((s.tokens || 0) / maxTokens) * 60);
    const color = s.status === 'running' ? 'var(--tx-2)' : 'var(--tx-4)';
    return `<div class="usage-timeline-bar" style="height:${h}px;background:${color}" title="${escHtml(s.prompt?.slice(0, 40) || 'Session')} — ${fmtTokens(s.tokens || 0)} tokens"></div>`;
  }).join('')}</div>`;
}

export function renderUsageTable(hist) {
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
