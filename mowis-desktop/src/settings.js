/**
 * MowisAI Desktop — Settings Page
 */

import { State, $, setText, toast, escHtml, syncCustomSelect, updateSettingsAgentHint } from './state.js';
import { invoke } from './bridge.js';

// ── Provider → Model mapping ─────────────────────────────────────────────────

export const PROVIDER_MODELS = {
  gemini: [
    { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro — frontier' },
    { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash — fast' },
    { id: 'gemini-2.5-flash-lite', label: 'Gemini 2.5 Flash-Lite — lightweight' },
    { id: 'gemini-2.0-flash', label: 'Gemini 2.0 Flash' },
  ],
  vertex: [
    { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro' },
    { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash' },
  ],
  anthropic: [
    { id: 'claude-opus-4-7', label: 'Claude Opus 4.7 — most capable' },
    { id: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6 — smart + fast' },
    { id: 'claude-haiku-4-5-20251001', label: 'Claude Haiku 4.5 — fast' },
  ],
  openai: [
    { id: 'gpt-4o', label: 'GPT-4o — flagship' },
    { id: 'o1', label: 'o1 — reasoning' },
    { id: 'o3-mini', label: 'o3-mini — fast reasoning' },
    { id: 'gpt-4o-mini', label: 'GPT-4o Mini — cheap' },
  ],
  grok: [
    { id: 'grok-3', label: 'Grok-3 — flagship' },
    { id: 'grok-3-fast', label: 'Grok-3 Fast' },
    { id: 'grok-3-mini', label: 'Grok-3 Mini' },
  ],
  groq: [
    { id: 'llama-3.3-70b-versatile', label: 'Llama 3.3 70B — versatile' },
    { id: 'llama-3.1-8b-instant', label: 'Llama 3.1 8B — instant' },
    { id: 'mixtral-8x7b-32768', label: 'Mixtral 8x7B' },
  ],
  mimo: [
    { id: 'mimo-v2.5-pro', label: 'MiMo-V2.5 Pro — 1T flagship' },
    { id: 'mimo-v2.5', label: 'MiMo-V2.5 — omnimodal' },
    { id: 'mimo-v2-pro', label: 'MiMo-V2 Pro' },
    { id: 'mimo-v2-omni-0327', label: 'MiMo-V2 Omni — see/hear/act' },
    { id: 'mimo-v2-flash', label: 'MiMo-V2 Flash — fast' },
    { id: 'mimo-v2.5-asr', label: 'MiMo-V2.5 ASR — speech recognition' },
    { id: 'mimo-v2.5-tts', label: 'MiMo-V2.5 TTS — voice' },
  ],
};

// ── Helpers ──────────────────────────────────────────────────────────────────

export function populateModelDropdown(provider) {
  const sel = $('set-model');
  const custom = $('set-model-custom');
  if (!sel) return;

  const models = PROVIDER_MODELS[provider] || [];
  sel.innerHTML = models.map(m => `<option value="${m.id}">${m.label}</option>`).join('')
    + '<option value="__custom">Custom…</option>';

  sel.onchange = () => {
    if (sel.value === '__custom') {
      custom?.classList.remove('hidden');
      custom?.focus();
    } else {
      custom?.classList.add('hidden');
    }
  };
}

export function setVal(id, val) {
  const e = $(id);
  if (!e) return;
  if (e.type === 'checkbox') e.checked = !!val;
  else e.value = val ?? '';
  if (e.tagName === 'SELECT') syncCustomSelect(e);
}

export function getVal(id) {
  const e = $(id);
  if (!e) return '';
  if (e.type === 'checkbox') return e.checked;
  return e.value;
}

// ── Load / Save ──────────────────────────────────────────────────────────────

export function loadSettings() {
  const c = State.config || {};
  setVal('set-provider', c.provider || 'gemini');
  populateModelDropdown(c.provider || 'gemini');

  const modelSel = $('set-model');
  const modelCustom = $('set-model-custom');
  const models = PROVIDER_MODELS[c.provider || 'gemini'] || [];
  const knownModel = models.some(m => m.id === c.model);
  if (c.model && knownModel && modelSel) {
    modelSel.value = c.model;
    modelCustom?.classList.add('hidden');
  } else if (c.model && modelSel) {
    modelSel.value = '__custom';
    if (modelCustom) { modelCustom.value = c.model; modelCustom.classList.remove('hidden'); }
  }

  setVal('set-api-key', c.api_key || '');
  setVal('set-gcp', c.gcp_project || '');
  setVal('set-gcp-region', c.gcp_region || 'us-central1');
  setVal('set-sa-key', c.gcp_service_account_key_path || '');
  setVal('set-socket', c.socket_path || '/tmp/agentd.sock');
  setVal('set-mode', c.mode || 'auto');
  setVal('set-max-agents', c.max_agents || 100);
  const rowGcp = $('row-gcp');
  const rowGcpRegion = $('row-gcp-region');
  const rowSaKey = $('row-sa-key');
  const showVertex = (c.provider === 'vertex');
  if (rowGcp) rowGcp.style.display = showVertex ? '' : 'none';
  if (rowGcpRegion) rowGcpRegion.style.display = showVertex ? '' : 'none';
  if (rowSaKey) rowSaKey.style.display = showVertex ? '' : 'none';

  const sandboxEnabled = c.sandbox_enabled !== false;
  setVal('set-sandbox-enabled', sandboxEnabled);
  updateSandboxToggleLabel(sandboxEnabled);

  updateSettingsAgentHint(c.mode || 'agent');

  refreshSandboxInfo();
}

export async function saveSettings() {
  const modelSel = $('set-model');
  const modelCustom = $('set-model-custom');
  let modelVal = modelSel?.value || '';
  if (modelVal === '__custom') modelVal = modelCustom?.value?.trim() || '';

  const config = {
    socket_path: getVal('set-socket'),
    max_agents: parseInt(getVal('set-max-agents') || '100'),
    mode: getVal('set-mode'),
    provider: getVal('set-provider'),
    model: modelVal,
    api_key: getVal('set-api-key'),
    gcp_project: getVal('set-gcp'),
    gcp_region: getVal('set-gcp-region'),
    gcp_service_account_key_path: getVal('set-sa-key'),
    sandbox_enabled: getVal('set-sandbox-enabled'),
  };
  try {
    await invoke('save_config', { config });
    State.config = config;
    setText('sb-provider', config.provider);
    toast('Settings saved', 'success');
  } catch (err) {
    toast('Save failed: ' + err, 'error');
  }
}

// ── Sandbox ──────────────────────────────────────────────────────────────────

export function updateSandboxToggleLabel(enabled) {
  const label = $('sandbox-toggle-label');
  if (label) label.textContent = enabled ? 'On' : 'Off';
}

export function fmtBytes(bytes) {
  if (bytes >= 1_073_741_824) return (bytes / 1_073_741_824).toFixed(1) + ' GB';
  if (bytes >= 1_048_576)     return (bytes / 1_048_576).toFixed(1) + ' MB';
  if (bytes >= 1_024)         return (bytes / 1_024).toFixed(1) + ' KB';
  return bytes + ' B';
}

export async function refreshSandboxInfo() {
  try {
    const info = await invoke('get_sandbox_status');
    const row = $('row-sandbox-info');
    const block = $('sandbox-info-block');
    const sizeEl = $('sandbox-size');
    if (!row || !block) return;

    if (!info) {
      row.style.display = 'none';
      return;
    }

    row.style.display = '';
    block.innerHTML = `
      <div class="sandbox-dir-row"><span class="sandbox-dir-label">lower</span><code class="sandbox-dir-path">${escHtml(info.lower_dir)}</code></div>
      <div class="sandbox-dir-row"><span class="sandbox-dir-label">upper</span><code class="sandbox-dir-path">${escHtml(info.upper_dir)}</code></div>
    `;

    try {
      const bytes = await invoke('get_sandbox_size');
      if (sizeEl) sizeEl.textContent = bytes > 0 ? fmtBytes(bytes) : '';
    } catch {}
  } catch {}
}

// ── Agent mode ───────────────────────────────────────────────────────────────

export function isAgentMode() {
  return (State.config?.mode || '') === 'agent' || State.agentHealthy;
}

export function updateZeroWorkspaceBar(path) {
  State.zeroWorkspacePath = path || null;
}

// ── Setup handlers ───────────────────────────────────────────────────────────

export function setupSettingsHandlers() {
  $('set-provider')?.addEventListener('change', (e) => {
    const rowGcp = $('row-gcp');
    const rowGcpRegion = $('row-gcp-region');
    const rowSaKey = $('row-sa-key');
    const showVertex = e.target.value === 'vertex';
    if (rowGcp) rowGcp.style.display = showVertex ? '' : 'none';
    if (rowGcpRegion) rowGcpRegion.style.display = showVertex ? '' : 'none';
    if (rowSaKey) rowSaKey.style.display = showVertex ? '' : 'none';
    populateModelDropdown(e.target.value);
    if (e.tagName === 'SELECT') syncCustomSelect(e);
  });
  $('set-mode')?.addEventListener('change', (e) => {
    updateSettingsAgentHint(e.target.value);
  });
  $('set-sandbox-enabled')?.addEventListener('change', (e) => {
    updateSandboxToggleLabel(e.target.checked);
  });
}
