/**
 * MowisAI Desktop — Pure utility functions
 */

export function delay(ms) { return new Promise(r => setTimeout(r, ms)); }

export function nowTs() { return Math.floor(Date.now() / 1000); }

export function fmtNumber(n) {
  if (!n) return '0';
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'k';
  return String(n);
}

export function fmtTs(ts) {
  if (!ts) return '—';
  const d = new Date(ts * 1000);
  return d.toLocaleDateString() + ' ' + d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

export function parseGitHubRepoName(repoUrl) {
  const raw = String(repoUrl || '').trim();
  const path = raw.startsWith('https://github.com/')
    ? raw.slice('https://github.com/'.length)
    : raw.startsWith('git@github.com:')
      ? raw.slice('git@github.com:'.length)
      : '';
  if (!path) return null;
  const clean = path.split(/[?#]/)[0].replace(/^\/+|\/+$/g, '').replace(/\.git$/, '');
  const parts = clean.split('/');
  if (parts.length !== 2 || !parts[0] || !parts[1]) return null;
  return { owner: parts[0], repo: parts[1] };
}
