export async function invoke(cmd, args = {}) {
  // Tauri v2 with withGlobalTauri: true
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke(cmd, args);
  }
  // Tauri v2 alternative path
  if (window.__TAURI_INTERNALS__?.invoke) {
    return window.__TAURI_INTERNALS__.invoke(cmd, args);
  }
  throw new Error('Tauri runtime not available — is this running inside the Tauri app?');
}

export async function listen(event, callback) {
  if (window.__TAURI__?.event?.listen) {
    return window.__TAURI__.event.listen(event, (e) => callback(e.payload));
  }
  return () => {};
}

export async function windowClose() {
  const win = window.__TAURI__?.window;
  if (win?.getCurrentWindow) {
    return win.getCurrentWindow().close();
  }
}

export async function windowMinimize() {
  const win = window.__TAURI__?.window;
  if (win?.getCurrentWindow) {
    return win.getCurrentWindow().minimize();
  }
}

export async function windowToggleMaximize() {
  const win = window.__TAURI__?.window;
  if (win?.getCurrentWindow) {
    return win.getCurrentWindow().toggleMaximize();
  }
}
