import { State, $, setText, escHtml, toast } from './state.js';
import { invoke } from './bridge.js';

export const SessionsState = {
  all: [],
  search: '',
};

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

export function renderSessionCard(s) {
  const relative = relativeTime(s.created_at || s.updated_at);
  const msgCount = s.message_count || 0;
  return `
    <div class="session-card" data-id="${escHtml(s.id)}">
      <button class="sc-delete" data-id="${escHtml(s.id)}" title="Delete session">&times;</button>
      <div class="sc-top">
        <div class="sc-prompt">${escHtml(s.title || '--')}</div>
        <div class="sc-badges">
          <span class="sc-status done">${msgCount} msgs</span>
        </div>
      </div>
      <div class="sc-meta">
        <span class="sc-meta-item">
          <svg width="11" height="11" viewBox="0 0 16 16" fill="none"><rect x="2" y="3" width="12" height="10" rx="1.5" stroke="currentColor" stroke-width="1.3"/><path d="M2 6h12" stroke="currentColor" stroke-width="1.3"/></svg>
          ${escHtml(relative)}
        </span>
        <span class="sc-meta-sep"></span>
        <span class="sc-meta-item">
          <svg width="11" height="11" viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.3"/><path d="M5 8h6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
          ${escHtml(s.id.slice(0, 8))}
        </span>
      </div>
    </div>`;
}

export async function renderSessionsPage() {
  try {
    const sessions = await invoke('agent_list_sessions');
    SessionsState.all = sessions || [];

    const list = $('sessions-list');
    const empty = $('sessions-empty');
    const toolbar = $('sessions-toolbar');

    setText('sb-badge-sessions', sessions.length ? String(sessions.length) : '');

    if (!sessions.length) {
      if (empty) empty.style.display = '';
      if (list) list.style.display = 'none';
      if (toolbar) toolbar.style.display = 'none';
      return;
    }

    if (empty) empty.style.display = 'none';
    if (toolbar) toolbar.style.display = '';
    setText('sessions-count', `${sessions.length} session${sessions.length !== 1 ? 's' : ''}`);

    renderSessionsList();
  } catch (e) {
    console.error('Failed to list sessions:', e);
  }
}

export function renderSessionsList() {
  const list = $('sessions-list');
  if (!list) return;

  const q = (SessionsState.search || '').toLowerCase();
  const filtered = SessionsState.all.filter(s => {
    if (!q) return true;
    return (s.title || '').toLowerCase().includes(q)
      || (s.id || '').toLowerCase().includes(q);
  });

  if (!filtered.length) {
    list.style.display = 'none';
    return;
  }

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
      try {
        await invoke('agent_delete_session', { session_id: id });
      } catch {}
      SessionsState.all = SessionsState.all.filter(s => s.id !== id);
      renderSessionsPage();
      toast('Session deleted', 'success');
    });
  });
}

async function openSession(sessionId) {
  try {
    const { navigate, loadSessionMessages } = await import('./main.js');
    State.sessionId = sessionId;
    loadSessionMessages(sessionId);
    navigate('home');
  } catch (err) {
    toast('Could not open session: ' + err, 'error');
  }
}

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

  $('btn-empty-new')?.addEventListener('click', async () => {
    const { navigate } = await import('./main.js');
    navigate('home');
  });
}
