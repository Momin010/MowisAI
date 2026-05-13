/**
 * MowisAI Desktop — Shared App State & Helpers
 */

export const State = {
  page: 'home',
  homeMode: 'new',
  sessionActive: false,
  sessionId: null,
  agentSessionId: null,
  taskPanelOpen: false,
  selectedTaskId: null,
  sidebarCollapsed: localStorage.getItem('mowis_sidebar_collapsed') === '1',
  tasks: {},
  streamingContent: '',
  isStreaming: false,
  daemonConnected: false,
  agentHealthy: false,
  config: null,
  selectedRepo: null,
  cloneDestination: null,
  setupError: null,
  stats: { tasks_total: 0, tasks_done: 0, tasks_running: 0, tokens_total: 0, tool_calls: 0 },
  zeroWorkspacePath: null,
  fileChanges: [],
  selectedChangePath: null,
  planCardShown: false,
  pollTimer: null,
  lastMessageCount: 0,
  diffTree: {
    query: '',
    actions: new Set(['created', 'modified', 'deleted', 'moved', 'read']),
    expanded: new Set(),
  },
};

export const $ = (id) => document.getElementById(id);
export const setText = (id, v) => { const e = $(id); if (e) e.textContent = v; };

export function toast(msg, type = 'info') {
  const c = $('toasts');
  if (!c) return;
  const t = document.createElement('div');
  t.className = `toast ${type}`;
  t.textContent = msg;
  c.appendChild(t);
  setTimeout(() => { t.style.opacity = '0'; t.style.transition = 'opacity 0.3s'; setTimeout(() => t.remove(), 320); }, 3200);
}

export async function promptSaveOutput({ prompt, fileChangeCount }) {
  // Check if there's a workspace to save
  let hasWorkspace = false;
  try {
    const { invoke: inv } = await import('./bridge.js');
    const ws = await inv('get_session_workspace');
    hasWorkspace = !!ws;
  } catch {}

  if (!hasWorkspace) return;

  const summary = fileChangeCount > 0
    ? `${fileChangeCount} file change${fileChangeCount !== 1 ? 's' : ''} ready to save.`
    : 'Your session has completed.';

  const confirmed = await showConfirm({
    title: 'Session complete — save output?',
    message: `${summary}\n\nWould you like to save the output files to your laptop?`,
    confirmLabel: 'Save to Laptop',
    cancelLabel: 'Not Now',
    danger: false,
  });

  if (!confirmed) return;

  // Open folder picker
  let destPath;
  try {
    const { openDialogNative } = await import('./bridge.js');
    destPath = await openDialogNative({ title: 'Choose destination folder', directory: true, multiple: false });
  } catch {}

  if (!destPath) return;

  try {
    const { invoke: inv } = await import('./bridge.js');
    await inv('export_workspace_to', { destination: Array.isArray(destPath) ? destPath[0] : destPath });
    toast('Output saved to ' + (Array.isArray(destPath) ? destPath[0] : destPath), 'success');
  } catch (e) {
    toast('Save failed: ' + e, 'error');
  }
}

export function showConfirm({ title, message, confirmLabel = 'Delete', cancelLabel = 'Cancel', danger = true }) {
  return new Promise((resolve) => {
    const overlay = document.createElement('div');
    overlay.className = 'confirm-overlay';
    overlay.innerHTML = `
      <div class="confirm-dialog" role="alertdialog" aria-modal="true">
        <div class="confirm-title">${escHtml(title)}</div>
        <div class="confirm-message">${escHtml(message)}</div>
        <div class="confirm-actions">
          <button class="btn-outline confirm-cancel">${escHtml(cancelLabel)}</button>
          <button class="btn-primary confirm-ok ${danger ? 'danger' : ''}">${escHtml(confirmLabel)}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const cleanup = (result) => { overlay.remove(); resolve(result); };
    overlay.querySelector('.confirm-ok').addEventListener('click', () => cleanup(true));
    overlay.querySelector('.confirm-cancel').addEventListener('click', () => cleanup(false));
    overlay.addEventListener('click', (e) => { if (e.target === overlay) cleanup(false); });
    overlay.querySelector('.confirm-ok').focus();
  });
}

export function escHtml(s) {
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

export function mdLite(text) {
  return escHtml(text)
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\n/g, '<br>');
}

// ── Sidebar ──────────────────────────────────────────────────────────────────

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

// ── Agent/Sandbox hint helpers ───────────────────────────────────────────────

export function updateHomeAgentHint(mode) {
  const hint = $('home-agent-hint');
  if (!hint) return;
  hint.classList.toggle('hidden', mode !== 'agent');
}

export function updateSettingsAgentHint(mode) {
  const hint = $('settings-agent-hint');
  if (!hint) return;
  hint.classList.toggle('hidden', mode !== 'agent');
}

// ── Repo chip ────────────────────────────────────────────────────────────────

export function renderRepoChip() {
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

// ── Custom select ────────────────────────────────────────────────────────────

export function syncCustomSelect(selectOrId) {
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
