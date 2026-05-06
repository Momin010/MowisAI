/**
 * MowisAI Desktop — Modals (Engine Setup, Repo, Developer Bootstrap, Diff Viewer)
 */

import { State, $, setText, toast, escHtml, renderRepoChip, updateHomeAgentHint, syncCustomSelect } from './state.js';
import { invoke, openDialogNative } from './bridge.js';
import { parseGitHubRepoName } from './utils.js';
import { setVal } from './settings.js';

// ── Engine setup modal ───────────────────────────────────────────────────────

export function hideEngineSetupModal() {
  const modal = $('engine-setup-modal');
  if (!modal) return;
  modal.classList.add('hidden');
  modal.setAttribute('aria-hidden', 'true');
}

function setModeToAgentAndReflectUI() {
  if (!State.config) State.config = {};
  State.config.mode = 'agent';

  const homeMode = $('home-mode');
  if (homeMode) {
    homeMode.value = 'agent';
    updateHomeAgentHint('agent');
    syncCustomSelect(homeMode);
  }

  const setMode = $('set-mode');
  if (setMode) {
    setMode.value = 'agent';
    syncCustomSelect(setMode);
  }
}

async function pickPathWithDialog({ title, directory = false }) {
  try {
    const selected = await openDialogNative({
      title,
      multiple: false,
      directory,
    });
    if (!selected) return null;
    return Array.isArray(selected) ? selected[0] : selected;
  } catch {
    return null;
  }
}

function normalizeQemuPathFromSelection(selection) {
  if (!selection) return '';
  const normalized = String(selection).replace(/\//g, '\\');
  if (normalized.toLowerCase().endsWith('.exe')) return normalized;
  return `${normalized.replace(/[\\\/]+$/, '')}\\qemu-system-x86_64.exe`;
}

export async function handlePointInstallationFlow() {
  const qemuSelection = await pickPathWithDialog({
    title: 'Select qemu-system-x86_64.exe or QEMU folder',
    directory: false,
  }) || await pickPathWithDialog({
    title: 'Select QEMU installation folder',
    directory: true,
  });

  if (!qemuSelection) {
    toast('Installation path selection cancelled', 'info');
    return;
  }

  const qemuPath = normalizeQemuPathFromSelection(qemuSelection);

  const defaultCfg = {
    qemu_path: qemuPath,
    iso_path: '',
    disk_path: '',
    mount_point: '/mnt/mowisai',
    disk_device: '/dev/sda',
    ram_mb: 512,
    agent_port: 8080,
    monitor_port: 4445,
    serial_port: 4444,
    agentd_path: '/mnt/mowisai/agentd',
  };

  try {
    const existing = await invoke('get_developer_config');
    const cfg = { ...existing, qemu_path: qemuPath };

    await invoke('save_developer_config', { config: cfg });
    hideEngineSetupModal();
    toast('QEMU path saved. Retrying engine startup...', 'success');
    setTimeout(() => window.location.reload(), 350);
  } catch (e) {
    toast(`Could not save installation: ${e}`, 'error');
  }
}

export function showEngineSetupModal(errorMessage) {
  const modal = $('engine-setup-modal');
  if (!modal) return;

  const statusMsg = $('engine-status-message');
  const errorBlock = $('engine-error');
  const errorMsg = $('engine-error-message');

  if (statusMsg) statusMsg.textContent = 'Could not detect WSL or QEMU on your system';
  if (errorMsg) errorMsg.textContent = errorMessage;
  if (errorBlock) errorBlock.classList.remove('hidden');

  modal.classList.remove('hidden');
  modal.setAttribute('aria-hidden', 'false');
}

export function setupEngineSetupModalHandlers() {
  const retryBtn = $('engine-retry');
  if (retryBtn) {
    retryBtn.onclick = () => {
      hideEngineSetupModal();
      window.location.reload();
    };
  }

  const continueBtn = $('engine-continue');
  if (continueBtn) {
    continueBtn.onclick = async () => {
      hideEngineSetupModal();
      setModeToAgentAndReflectUI();
      try { await invoke('save_config', { config: State.config }); } catch {}
      toast('Continuing in Agent mode', 'success');
    };
  }

  const manualBtn = $('engine-manual');
  if (manualBtn) {
    manualBtn.onclick = async () => {
      toast('Point to your QEMU installation', 'info');
      await handlePointInstallationFlow();
    };
  }

  const helpWsl = $('engine-help-wsl');
  if (helpWsl) {
    helpWsl.onclick = (e) => {
      e.preventDefault();
      toast('Open PowerShell (Admin) and run: wsl --install', 'info');
    };
  }

  const helpQemu = $('engine-help-qemu');
  if (helpQemu) {
    helpQemu.onclick = (e) => {
      e.preventDefault();
      toast('Install QEMU, then restart the app', 'info');
    };
  }
}

// ── Repo modal ───────────────────────────────────────────────────────────────

function setRepoStatus(message, type = 'info') {
  const status = $('repo-status');
  if (!status) return;
  status.textContent = message || '';
  status.className = `repo-status ${type}`;
}

function setRepoBusy(isBusy) {
  ['repo-pick-local', 'repo-pick-destination', 'repo-clone', 'repo-use'].forEach(id => {
    const el = $(id);
    if (el) el.disabled = !!isBusy;
  });
}

export function showRepoModal() {
  const modal = $('repo-modal');
  if (!modal) return;
  modal.classList.remove('hidden');
  modal.setAttribute('aria-hidden', 'false');
  setRepoStatus('');
  if (!State.selectedRepo) {
    $('repo-selected')?.classList.add('hidden');
    const urlInput = $('repo-url');
    if (urlInput) urlInput.value = '';
    setText('repo-destination-label', 'No folder selected');
    State.cloneDestination = null;
  }
  $('repo-url')?.focus();
}

export function hideRepoModal() {
  const modal = $('repo-modal');
  if (!modal) return;
  modal.classList.add('hidden');
  modal.setAttribute('aria-hidden', 'true');
}

export function setRepoTab(tab) {
  document.querySelectorAll('.repo-tab').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.repoTab === tab);
  });
  document.querySelectorAll('.repo-panel').forEach(panel => {
    panel.classList.toggle('active', panel.id === `repo-panel-${tab}`);
  });
  setRepoStatus('');
}

export function stageSelectedRepo(info) {
  State.selectedRepo = info;
  $('repo-selected')?.classList.remove('hidden');
  setText('repo-selected-name', info?.name || 'repository');
  setText('repo-selected-path', info?.path || '');
  renderRepoChip();
}

async function pickDirectory() {
  try {
    const picked = await openDialogNative({ directory: true, multiple: false });
    return Array.isArray(picked) ? picked[0] : picked;
  } catch {
    return null;
  }
}

export async function handlePickLocalRepo() {
  try {
    const path = await pickDirectory();
    if (!path) return;
    setRepoBusy(true);
    setRepoStatus('Checking repository...');
    const info = await invoke('validate_git_repository', { path });
    stageSelectedRepo(info);
    setRepoStatus('Repository ready', 'success');
  } catch (err) {
    setRepoStatus(String(err), 'error');
  } finally {
    setRepoBusy(false);
  }
}

export async function handlePickCloneDestination() {
  try {
    const path = await pickDirectory();
    if (!path) return;
    State.cloneDestination = path;
    setText('repo-destination-label', path);
    setRepoStatus('');
  } catch (err) {
    setRepoStatus(String(err), 'error');
  }
}

export async function handleCloneGitHubRepo() {
  const repoUrl = $('repo-url')?.value.trim();
  if (!parseGitHubRepoName(repoUrl)) {
    setRepoStatus('Paste a GitHub URL like https://github.com/owner/repo', 'error');
    return;
  }
  if (!State.cloneDestination) {
    setRepoStatus('Choose a destination folder', 'error');
    return;
  }

  try {
    setRepoBusy(true);
    setRepoStatus('Cloning repository...');
    const info = await invoke('clone_github_repo', {
      repoUrl,
      destinationParent: State.cloneDestination,
    });
    stageSelectedRepo(info);
    setRepoStatus('Repository cloned', 'success');
  } catch (err) {
    setRepoStatus(String(err), 'error');
  } finally {
    setRepoBusy(false);
  }
}

export function useSelectedRepo() {
  if (!State.selectedRepo) {
    setRepoStatus('Select or clone a repository first', 'error');
    return;
  }
  renderRepoChip();
  hideRepoModal();
  toast('Repository attached', 'success');
}

export function setupRepoHandlers() {
  $('btn-repo-open')?.addEventListener('click', showRepoModal);
  $('repo-close')?.addEventListener('click', hideRepoModal);
  $('repo-backdrop')?.addEventListener('click', hideRepoModal);
  $('repo-pick-local')?.addEventListener('click', handlePickLocalRepo);
  $('repo-pick-destination')?.addEventListener('click', handlePickCloneDestination);
  $('repo-clone')?.addEventListener('click', handleCloneGitHubRepo);
  $('repo-use')?.addEventListener('click', useSelectedRepo);
  $('repo-remove')?.addEventListener('click', () => {
    State.selectedRepo = null;
    State.cloneDestination = null;
    $('repo-selected')?.classList.add('hidden');
    const urlInput = $('repo-url');
    if (urlInput) urlInput.value = '';
    setText('repo-destination-label', 'No folder selected');
    setRepoStatus('');
    renderRepoChip();
  });
  document.querySelectorAll('.repo-tab').forEach(tab => {
    tab.addEventListener('click', () => setRepoTab(tab.dataset.repoTab));
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !$('repo-modal')?.classList.contains('hidden')) hideRepoModal();
  });
}

// ── Developer bootstrap ──────────────────────────────────────────────────────

export function showDeveloperBootstrap() {
  const modal = document.getElementById('developer-bootstrap-modal');
  if (!modal) return;
  modal.classList.remove('hidden');
  modal.setAttribute('aria-hidden', 'false');

  const statusEl = document.getElementById('dev-bs-status');
  if (statusEl) { statusEl.classList.add('hidden'); statusEl.textContent = ''; }

  invoke('get_developer_config').then(cfg => {
    setVal('dev-bs-qemu', cfg.qemu_path);
    setVal('dev-bs-iso', cfg.iso_path);
    setVal('dev-bs-disk', cfg.disk_path);
    setVal('dev-bs-port', cfg.agent_port || 8080);
  }).catch(() => {});
}

function hideDeveloperBootstrap() {
  const modal = document.getElementById('developer-bootstrap-modal');
  if (!modal) return;
  modal.classList.add('hidden');
  modal.setAttribute('aria-hidden', 'true');
}

async function startDeveloperBootstrap() {
  const qemu = document.getElementById('dev-bs-qemu')?.value?.trim();
  const iso  = document.getElementById('dev-bs-iso')?.value?.trim();
  const disk = document.getElementById('dev-bs-disk')?.value?.trim();
  const port = parseInt(document.getElementById('dev-bs-port')?.value || '8080', 10);

  if (!qemu || !iso || !disk) {
    toast('All three paths (QEMU, ISO, Disk) are required', 'error');
    return;
  }

  const statusEl = document.getElementById('dev-bs-status');
  if (statusEl) {
    statusEl.classList.remove('hidden');
    statusEl.textContent = 'Saving configuration...';
    statusEl.className = 'dev-bootstrap-status';
  }

  const config = {
    qemu_path: qemu,
    iso_path: iso,
    disk_path: disk,
    ram_mb: 512,
    agent_port: port,
    monitor_port: 4445,
    serial_port: 4444,
    mount_point: '/mnt/mowisai',
    disk_device: '/dev/sda',
    agentd_path: '/mnt/mowisai/agentd',
  };

  try {
    const result = await invoke('start_developer_bootstrap', { config });

    if (statusEl) {
      statusEl.textContent = 'Config saved. Restarting...';
      statusEl.className = 'dev-bootstrap-status ok';
    }

    toast('Configuration saved. Restarting to bootstrap QEMU...', 'success');

    setTimeout(() => window.location.reload(), 1500);
  } catch (e) {
    if (statusEl) {
      statusEl.textContent = String(e);
      statusEl.className = 'dev-bootstrap-status err';
    }
    toast('Bootstrap failed: ' + e, 'error');
  }
}

export function setupDeveloperBootstrapHandlers() {
  const cancelBtn = document.getElementById('dev-bs-cancel');
  if (cancelBtn) {
    cancelBtn.addEventListener('click', hideDeveloperBootstrap);
  }

  const startBtn = document.getElementById('dev-bs-start');
  if (startBtn) {
    startBtn.addEventListener('click', startDeveloperBootstrap);
  }

  const browseBtns = [
    { btnId: 'dev-bs-browse-qemu', inputId: 'dev-bs-qemu', title: 'Select QEMU Binary' },
    { btnId: 'dev-bs-browse-iso',  inputId: 'dev-bs-iso',  title: 'Select ISO File' },
    { btnId: 'dev-bs-browse-disk', inputId: 'dev-bs-disk', title: 'Select Disk Image' },
  ];
  browseBtns.forEach(({ btnId, inputId, title }) => {
    const btn = document.getElementById(btnId);
    if (btn) {
      btn.addEventListener('click', async () => {
        try {
          const selected = await openDialogNative({ title, multiple: false, directory: false });
          if (selected) {
            const input = document.getElementById(inputId);
            if (input) input.value = Array.isArray(selected) ? selected[0] : selected;
          }
        } catch (e) {
          console.error('File dialog error:', e);
        }
      });
    }
  });

  const bootstrapModal = document.getElementById('developer-bootstrap-modal');
  if (bootstrapModal) {
    const backdrop = bootstrapModal.querySelector('.engine-modal-backdrop');
    if (backdrop) {
      backdrop.addEventListener('click', hideDeveloperBootstrap);
    }
  }
}
