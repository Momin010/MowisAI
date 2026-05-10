export async function invoke(cmd, args = {}) {
  if (window.__TAURI__?.core?.invoke) return window.__TAURI__.core.invoke(cmd, args);
  throw new Error('Tauri not available');
}

export async function listen(event, callback) {
  if (window.__TAURI__?.event?.listen) return window.__TAURI__.event.listen(event, (e) => callback(e.payload));
  return () => {};
}
