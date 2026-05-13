/**
 * MowisAI Desktop — Sessions Page
 */

import { State, $, setText, escHtml, toast } from './state.js';
import { invoke } from './bridge.js';
import { fmtNumber } from './utils.js';

export const SessionsState = {
  all: [],
  search: '',
  filter: 'all',
  sort: 'newest',
};

// ── Helpers ──────────────────────────────────────────────────────────────────

export function relativeTime(ts) {
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

export function formatDuration(secs) {
  if (!secs || secs <= 0) return null;
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const s = secs % 60;
  if (mins < 60) return s > 0 ? `${mins}m ${s}s` : `${mins}m`;
  const hrs = Math.floor(mins / 60);
  const m = mins % 60;
  return m > 0 ? `${hrs}h ${m}m` : `${hrs}h`;
}

export function sessionsMatchFilter(s, filter) {
  if (filter === 'all') return true;
  return s.status === filter;
}

export function sessionsMatchSearch(s, query) {
  if (!query) return true;
  const q = query.toLowerCase();
  return (s.prompt || '').toLowerCase().includes(q)
    || (s.id || '').toLowerCase().includes(q)
    || relativeTime(s.started_at).toLowerCase().includes(q);
}

export function sessionsSort(a, b, sort) {
  switch (sort) {
    case 'oldest': return a.started_at - b.started_at;
    case 'tokens': return (b.tokens_total || 0) - (a.tokens_total || 0);
    case 'duration': return (b.duration_secs || 0) - (a.duration_secs || 0);
    case 'newest':
    default: return b.started_at - a.started_at;
  }
}

// ── Rendering ────────────────────────────────────────────────────────────────

export function renderSessionCard(s) {
  const progressPct = s.task_count > 0 ? Math.round((s.tasks_done / s.task_count) * 100) : 0;
  const progressClass = s.status === 'error' ? 'error' : s.status === 'done' ? 'complete' : '';
  const duration = formatDuration(s.duration_secs);
  const relative = relativeTime(s.started_at);
  const statusLabel = s.status === 'done' ? 'COMPLETED' : s.status.toUpperCase();

  return `
    <div class="session-card" data-id="${s.id}">
      <button class="sc-delete" data-id="${s.id}" title="Delete session" aria-label="Delete session">&times;</button>
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

export async function renderSessionsPage() {
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

export function renderSessionsActive(activeEl) {
  if (!activeEl) return;
  const running = SessionsState.all.find(s => s.status === 'running');
  if (!running) {
    activeEl.style.display = 'none';
    return;
  }
  activeEl.style.display = '';
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

export function renderSessionsList() {
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
    card.addEventListener('click', (e) => {
      if (e.target.closest('.sc-delete')) return;
      openSession(card.dataset.id);
    });
  });

  list.querySelectorAll('.sc-delete').forEach(btn => {
    btn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const id = btn.dataset.id;
      if (!id) return;

      if (!confirm('Delete this session? This cannot be undone.')) return;

      try {
        await invoke('delete_session', { sessionId: id });
      } catch (err) {
        toast('Failed to delete: ' + err, 'error');
        return;
      }

      SessionsState.all = SessionsState.all.filter(s => s.id !== id);
      renderSessionsPage();
      toast('Session deleted', 'success');
    });
  });
}

// ── Session actions ──────────────────────────────────────────────────────────

async function openSession(sessionId) {
  try {
    const detail = await invoke('load_session', { sessionId });
    const { navigate, renderSessionDetail } = await import('./main.js');
    renderSessionDetail(detail);
    navigate('home', { preserveHomeMode: true });
  } catch (err) {
    toast('Could not open session: ' + err, 'error');
  }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

export function setupSessionsHandlers() {
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

  document.querySelectorAll('.sessions-filter').forEach(btn => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.sessions-filter').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      SessionsState.filter = btn.dataset.filter;
      renderSessionsList();
    });
  });

  const sortSelect = $('sessions-sort');
  if (sortSelect) {
    sortSelect.addEventListener('change', () => {
      SessionsState.sort = sortSelect.value;
      renderSessionsList();
    });
  }

  // Dynamic import to avoid circular dependency with main.js
  $('btn-empty-new')?.addEventListener('click', async () => {
    const { navigate } = await import('./main.js');
    navigate('home');
  });

  // Auto-refresh every 5 seconds so externally-deleted sessions disappear
  if (!window._sessionsRefreshTimer) {
    window._sessionsRefreshTimer = setInterval(() => {
      if (document.getElementById('page-sessions')?.classList.contains('active')) {
        renderSessionsPage();
      }
    }, 5000);
  }
}
