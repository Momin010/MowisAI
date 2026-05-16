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

    // Bridge: subscribe to stream_event from Rust and fan out as window CustomEvents.
    // This is the live-streaming pipeline: agentd → Rust → Tauri → JS CustomEvent → chat.js
    _listen('stream_event', (e) => {
      let ev;
      try {
        ev = typeof e.payload === 'string' ? JSON.parse(e.payload) : e.payload;
      } catch { return; }
      const type = ev && (ev.type || ev.event_type || '');
      if (!type) return;
      const detail = ev;
      if (type === 'LlmChunk') {
        window.dispatchEvent(new CustomEvent('mowis:llm_chunk', { detail }));
      } else if (type === 'LlmDone') {
        window.dispatchEvent(new CustomEvent('mowis:llm_done', { detail }));
      } else if (type === 'ToolCall') {
        window.dispatchEvent(new CustomEvent('mowis:tool_call', { detail }));
      } else if (type === 'ToolResult') {
        window.dispatchEvent(new CustomEvent('mowis:tool_result', { detail }));
      } else if (type === 'AgentStarted' || type === 'AgentCompleted' || type === 'AgentFailed') {
        window.dispatchEvent(new CustomEvent('mowis:agent_state', { detail }));
      } else if (type === 'LayerStarted' || type === 'LayerCompleted') {
        window.dispatchEvent(new CustomEvent('mowis:layer_progress', { detail }));
      } else if (type === 'SessionComplete') {
        window.dispatchEvent(new CustomEvent('mowis:session_complete', { detail }));
      }
    }).catch(() => {});

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
