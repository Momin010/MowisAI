/**
 * MowisAI Desktop — Tauri bridge
 * Provides invoke/listen/openDialog with graceful browser fallback.
 */

import { mockInvoke } from './mock.js';

let _invoke = null;
let _listen = null;
let _openDialog = null;

export async function loadTauri() {
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen }  = await import('@tauri-apps/api/event');
    const { open } = await import('@tauri-apps/plugin-dialog');
    _invoke = invoke;
    _listen = listen;
    _openDialog = open;
    return true;
  } catch { return false; }
}

export function isTauri() { return !!_invoke; }

export async function invoke(cmd, args = {}) {
  if (_invoke) return _invoke(cmd, args);
  return mockInvoke(cmd, args);
}

export async function listen(event, cb) {
  if (_listen) return _listen(event, cb);
  // Browser: events are dispatched via dispatchEvent below
  const handler = (e) => cb(e.detail);
  window.addEventListener(`tauri:${event}`, handler);
  return () => window.removeEventListener(`tauri:${event}`, handler);
}

export function openDialogNative(opts) {
  if (_openDialog) return _openDialog(opts);
  return Promise.resolve(null);
}

export function dispatchMockEvent(event, payload) {
  window.dispatchEvent(new CustomEvent(`tauri:${event}`, { detail: { payload } }));
}
