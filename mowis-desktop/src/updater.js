/**
 * MowisAI Desktop — Auto-updater
 * Checks for updates via GitHub Releases. Shows an in-app notification bar
 * when a new version is available, downloads in the background, and prompts
 * the user to relaunch — same UX as the Claude Desktop app.
 */

import { isTauri } from './bridge.js';
import { $ } from './state.js';

let updateBanner = null;

function createBanner() {
  if (updateBanner) return updateBanner;
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
      <span class="update-banner-text" id="update-banner-text">A new update is available.</span>
      <div class="update-banner-actions">
        <button class="update-banner-btn primary" id="btn-update-action" style="display:none">Relaunch</button>
        <button class="update-banner-btn dismiss" id="btn-update-dismiss">Dismiss</button>
      </div>
    </div>
    <div class="update-banner-progress hidden" id="update-progress">
      <div class="update-banner-progress-fill" id="update-progress-fill"></div>
    </div>`;
  document.body.prepend(el);
  updateBanner = el;

  $('btn-update-dismiss')?.addEventListener('click', () => {
    el.classList.add('hidden');
  });

  return el;
}

function showBanner(text, showAction) {
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
  if (fill) fill.style.width = Math.round(pct) + '%';
}

function hideProgress() {
  const bar = $('update-progress');
  if (bar) bar.classList.add('hidden');
}

/**
 * Check for updates and drive the full download-and-relaunch flow.
 * Call this once during app init (after splash).
 */
export async function initUpdater() {
  if (!isTauri()) return;

  // Delay the check so boot isn't blocked
  setTimeout(async () => {
    try {
      await checkAndUpdate();
    } catch (e) {
      console.warn('[updater] Update check failed:', e);
    }
  }, 5000);
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
  const update = await check();

  if (!update) {
    console.log('[updater] App is up to date.');
    return;
  }

  console.log('[updater] Update available:', update.version);
  showBanner(`MowisAI v${update.version} is available. Downloading...`, false);

  let totalBytes = 0;
  let downloadedBytes = 0;

  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          totalBytes = event.data?.contentLength || 0;
          downloadedBytes = 0;
          if (totalBytes > 0) setProgress(0);
          break;
        case 'Progress':
          downloadedBytes += event.data?.chunkLength || 0;
          if (totalBytes > 0) {
            setProgress((downloadedBytes / totalBytes) * 100);
          }
          break;
        case 'Finished':
          hideProgress();
          break;
      }
    });
  } catch (e) {
    console.error('[updater] Download failed:', e);
    hideProgress();
    showBanner('Update download failed. Will retry next launch.', false);
    return;
  }

  // Update downloaded and installed — prompt user to relaunch
  const actionBtn = $('btn-update-action');
  showBanner(`MowisAI v${update.version} has been downloaded. Relaunch to apply.`, true);

  if (actionBtn) {
    actionBtn.addEventListener('click', async () => {
      actionBtn.disabled = true;
      actionBtn.textContent = 'Relaunching...';
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
