/**
 * MowisAI Desktop — Auto-updater
 * Checks for updates via GitHub Releases. Shows a modal asking the user
 * whether to install, downloads with progress, then prompts to relaunch.
 */

import { isTauri } from './bridge.js';
import { $, showConfirm } from './state.js';

function createBanner() {
  const existing = $('update-banner');
  if (existing) return existing;
  const el = document.createElement('div');
  el.id = 'update-banner';
  el.className = 'update-banner hidden';
  el.innerHTML = `
    <div class="update-banner-content">
      <span class="update-banner-icon">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="17 1 21 5 17 9"></polyline>
          <path d="M3 11V9a4 4 0 0 1 4-4h14"></path>
          <polyline points="7 23 3 19 7 15"></polyline>
          <path d="M21 13v2a4 4 0 0 1-4 4H3"></path>
        </svg>
      </span>
      <span class="update-banner-text" id="update-banner-text"></span>
      <div class="update-banner-actions">
        <button class="update-banner-btn primary" id="btn-update-action" style="display:none">Relaunch</button>
        <button class="update-banner-btn dismiss" id="btn-update-dismiss">Dismiss</button>
      </div>
    </div>
    <div class="update-banner-progress hidden" id="update-progress">
      <div class="update-banner-progress-fill" id="update-progress-fill"></div>
    </div>`;
  document.body.prepend(el);
  $('btn-update-dismiss')?.addEventListener('click', () => el.classList.add('hidden'));
  return el;
}

function showBanner(text, showAction = false) {
  const banner = createBanner();
  const textEl = $('update-banner-text');
  const actionBtn = $('btn-update-action');
  if (textEl) textEl.textContent = text;
  if (actionBtn) actionBtn.style.display = showAction ? '' : 'none';
  banner.classList.remove('hidden');
}

function setProgress(pct) {
  const bar = $('update-progress');
  const fill = $('update-progress-fill');
  if (bar) bar.classList.remove('hidden');
  if (fill) fill.style.width = `${Math.round(pct)}%`;
}

function hideProgress() {
  $('update-progress')?.classList.add('hidden');
}

/**
 * Check for updates and drive the full download-and-relaunch flow.
 * Call this once during app init (after splash).
 */
export async function initUpdater() {
  if (!isTauri()) return;

  // Delay so boot is not blocked
  setTimeout(async () => {
    try {
      await checkAndUpdate();
    } catch (e) {
      console.warn('[updater] Update check failed:', e);
    }
  }, 8000);
}

async function checkAndUpdate() {
  let check, relaunch;
  try {
    const updater = await import('@tauri-apps/plugin-updater');
    const process = await import('@tauri-apps/plugin-process');
    check = updater.check;
    relaunch = process.relaunch;
  } catch (e) {
    console.warn('[updater] Plugin not available:', e);
    return;
  }

  console.log('[updater] Checking for updates...');
  let update;
  try {
    update = await check();
  } catch (e) {
    console.warn('[updater] check() failed:', e);
    return;
  }

  if (!update) {
    console.log('[updater] App is up to date.');
    return;
  }

  console.log('[updater] Update available:', update.version);

  // Ask the user first — prominent modal, not a subtle banner
  const confirmed = await showConfirm({
    title: `MowisAI ${update.version} is available`,
    message: 'A new version is ready to install. It will download in the background and apply on the next relaunch. Install now?',
    confirmLabel: 'Install Update',
    cancelLabel: 'Later',
    danger: false,
  });

  if (!confirmed) {
    console.log('[updater] User deferred update.');
    return;
  }

  showBanner(`Downloading MowisAI v${update.version}…`);

  let totalBytes = 0;
  let downloadedBytes = 0;

  try {
    await update.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        totalBytes = event.data?.contentLength || 0;
        downloadedBytes = 0;
        if (totalBytes > 0) setProgress(0);
      } else if (event.event === 'Progress') {
        downloadedBytes += event.data?.chunkLength || 0;
        if (totalBytes > 0) setProgress((downloadedBytes / totalBytes) * 100);
      } else if (event.event === 'Finished') {
        hideProgress();
      }
    });
  } catch (e) {
    console.error('[updater] Download failed:', e);
    hideProgress();
    showBanner('Update download failed. It will retry on the next launch.');
    return;
  }

  // Downloaded — prompt to relaunch
  showBanner(`MowisAI v${update.version} installed. Relaunch to apply.`, true);

  const actionBtn = $('btn-update-action');
  if (actionBtn) {
    actionBtn.addEventListener('click', async () => {
      actionBtn.disabled = true;
      actionBtn.textContent = 'Relaunching…';
      try {
        await relaunch();
      } catch (e) {
        console.error('[updater] Relaunch failed:', e);
        actionBtn.textContent = 'Relaunch';
        actionBtn.disabled = false;
      }
    }, { once: true });
  }
}
