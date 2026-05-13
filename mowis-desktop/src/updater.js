/**
 * MowisAI Desktop — Auto-updater
 *
 * Polls GitHub Releases API silently in the background.
 * Shows a confirm modal only when a newer version is found.
 * Completely silent otherwise — no banners, no spinners.
 */

import { isTauri } from './bridge.js';
import { $, showConfirm } from './state.js';

const GITHUB_API = 'https://api.github.com/repos/Momin010/MowisAI/releases/latest';
const CHECK_INTERVAL_MS = 30 * 60 * 1000; // 30 minutes
let _alreadyPrompted = false;
let _currentVersion = null;

function parseSemver(tag) {
  // accepts "v0.2.4" or "0.2.4"
  const m = tag.replace(/^v/, '').match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!m) return null;
  return [parseInt(m[1], 10), parseInt(m[2], 10), parseInt(m[3], 10)];
}

function isNewer(remote, local) {
  if (!remote || !local) return false;
  for (let i = 0; i < 3; i++) {
    if (remote[i] > local[i]) return true;
    if (remote[i] < local[i]) return false;
  }
  return false;
}

async function getCurrentVersion() {
  if (_currentVersion) return _currentVersion;
  try {
    const { getVersion } = await import('@tauri-apps/api/app');
    _currentVersion = await getVersion();
  } catch {
    // fallback: read from tauri config injected at build time
    _currentVersion = window.__TAURI_METADATA__?.tauriVersion || '0.0.0';
  }
  return _currentVersion;
}

async function checkOnce() {
  try {
    const res = await fetch(GITHUB_API, {
      headers: { Accept: 'application/vnd.github+json' },
    });
    if (!res.ok) return;
    const data = await res.json();
    const remoteTag = data?.tag_name;
    if (!remoteTag) return;

    const remote = parseSemver(remoteTag);
    const localStr = await getCurrentVersion();
    const local = parseSemver(localStr);

    if (!isNewer(remote, local)) return;

    // Found a newer version — show modal once per session
    if (_alreadyPrompted) return;
    _alreadyPrompted = true;

    const remoteVersion = remoteTag.replace(/^v/, '');
    const confirmed = await showConfirm({
      title: `MowisAI ${remoteVersion} is available`,
      message: `You are running ${localStr}. Download the new version from the releases page?`,
      confirmLabel: 'Open Releases',
      cancelLabel: 'Later',
      danger: false,
    });

    if (!confirmed) return;

    // Open the GitHub releases page in the default browser
    const releaseUrl = data.html_url || `https://github.com/Momin010/MowisAI/releases/latest`;
    try {
      const { invoke: inv } = await import('./bridge.js');
      await inv('open_url', { url: releaseUrl });
    } catch {
      window.open(releaseUrl, '_blank');
    }
  } catch (e) {
    console.warn('[updater] check failed:', e);
  }
}

export function initUpdater() {
  if (!isTauri()) return;

  // First check after a short boot delay
  setTimeout(checkOnce, 12000);

  // Then repeat every 30 minutes
  setInterval(checkOnce, CHECK_INTERVAL_MS);
}
