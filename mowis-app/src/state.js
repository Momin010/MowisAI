export const State = {
  page: 'home',
  config: {},
  sessionId: null,
  sessions: [],
  messages: [],
  agentHealthy: false,
  agentRunning: false,
  sessionActive: false,
  pollTimer: null,
  lastMessageCount: 0,
  sidebarCollapsed: localStorage.getItem('mowis_sidebar_collapsed') === '1',
};

export function $(id) { return document.getElementById(id); }

export function setText(id, text) {
  const el = $(id);
  if (el) el.textContent = text;
}

export function escHtml(s) {
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

export function mdLite(text) {
  return escHtml(text)
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\n/g, '<br>');
}

export function toast(msg, type = 'info') {
  const c = $('toasts');
  if (!c) return;
  const t = document.createElement('div');
  t.className = `toast ${type}`;
  t.textContent = msg;
  c.appendChild(t);
  setTimeout(() => {
    t.style.opacity = '0';
    t.style.transition = 'opacity 0.3s';
    setTimeout(() => t.remove(), 320);
  }, 3200);
}

export function setSidebarCollapsed(collapsed) {
  State.sidebarCollapsed = !!collapsed;
  document.querySelector('.layout')?.classList.toggle('sidebar-collapsed', State.sidebarCollapsed);
  localStorage.setItem('mowis_sidebar_collapsed', State.sidebarCollapsed ? '1' : '0');
  const btn = $('btn-sidebar-toggle');
  if (btn) {
    btn.title = State.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar';
    btn.setAttribute('aria-label', btn.title);
  }
}

export function nowTs() { return Date.now(); }
