import { State, $, setText, toast, escHtml } from './state.js';
import { invoke } from './bridge.js';

export const PROVIDER_MODELS = {
  gemini: [
    { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro -- frontier' },
    { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash -- fast' },
    { id: 'gemini-2.5-flash-lite', label: 'Gemini 2.5 Flash-Lite -- lightweight' },
    { id: 'gemini-2.0-flash', label: 'Gemini 2.0 Flash' },
  ],
  vertexai: [
    { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro' },
    { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash' },
  ],
  anthropic: [
    { id: 'claude-opus-4-7', label: 'Claude Opus 4.7 -- most capable' },
    { id: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6 -- smart + fast' },
    { id: 'claude-haiku-4-5-20251001', label: 'Claude Haiku 4.5 -- fast' },
  ],
  openai: [
    { id: 'gpt-4o', label: 'GPT-4o -- flagship' },
    { id: 'o1', label: 'o1 -- reasoning' },
    { id: 'o3-mini', label: 'o3-mini -- fast reasoning' },
    { id: 'gpt-4o-mini', label: 'GPT-4o Mini -- cheap' },
  ],
  xai: [
    { id: 'grok-3', label: 'Grok-3 -- flagship' },
    { id: 'grok-3-fast', label: 'Grok-3 Fast' },
    { id: 'grok-3-mini', label: 'Grok-3 Mini' },
  ],
  groq: [
    { id: 'llama-3.3-70b-versatile', label: 'Llama 3.3 70B -- versatile' },
    { id: 'llama-3.1-8b-instant', label: 'Llama 3.1 8B -- instant' },
    { id: 'mixtral-8x7b-32768', label: 'Mixtral 8x7B' },
  ],
  openrouter: [
    { id: 'anthropic/claude-sonnet-4', label: 'Claude Sonnet 4 (OpenRouter)' },
    { id: 'google/gemini-2.5-flash', label: 'Gemini 2.5 Flash (OpenRouter)' },
  ],
};

export function populateModelDropdown(provider) {
  const sel = $('set-model');
  if (!sel) return;
  const models = PROVIDER_MODELS[provider] || [];
  sel.innerHTML = models.map(m => `<option value="${m.id}">${m.label}</option>`).join('');
  if (models.length === 0) {
    sel.innerHTML = '<option value="">No presets available</option>';
  }
}

export function loadSettings() {
  const c = State.config || {};
  const providerSel = $('set-provider');
  if (providerSel) providerSel.value = c.provider || 'gemini';
  populateModelDropdown(c.provider || 'gemini');

  const modelSel = $('set-model');
  if (modelSel && c.model) modelSel.value = c.model;

  const keyInput = $('set-api-key');
  if (keyInput) keyInput.value = c.api_key || '';

  const gcpInput = $('set-gcp');
  if (gcpInput) gcpInput.value = c.gcp_project || '';

  const cwdInput = $('set-cwd');
  if (cwdInput) cwdInput.value = c.cwd || '';

  updateProviderFields(c.provider || 'gemini');
}

function updateProviderFields(provider) {
  const rowKey = $('row-api-key');
  const rowGcp = $('row-gcp');
  const needsKey = provider !== 'vertexai';
  const needsGcp = provider === 'vertexai';
  if (rowKey) rowKey.style.display = needsKey ? '' : 'none';
  if (rowGcp) rowGcp.style.display = needsGcp ? '' : 'none';
}

export async function saveSettings() {
  const provider = $('set-provider')?.value || 'gemini';
  const model = $('set-model')?.value || '';
  const apiKey = $('set-api-key')?.value?.trim() || '';
  const gcpProject = $('set-gcp')?.value?.trim() || '';
  const cwd = $('set-cwd')?.value?.trim() || '';

  if (provider !== 'vertexai' && !apiKey) {
    toast('Enter an API key', 'error');
    return;
  }
  if (provider === 'vertexai' && !gcpProject) {
    toast('Enter a GCP Project ID', 'error');
    return;
  }

  const config = {
    agent_port: State.config.agent_port || 4096,
    provider,
    model,
    api_key: apiKey,
    gcp_project: gcpProject,
    cwd,
  };

  try {
    await invoke('save_agent_config', { config });
    State.config = config;
    setText('sb-provider', provider);
    setText('status-provider', provider);
    toast('Settings saved', 'success');
  } catch (err) {
    toast('Save failed: ' + err, 'error');
  }
}

export function setupSettingsHandlers() {
  $('set-provider')?.addEventListener('change', (e) => {
    updateProviderFields(e.target.value);
    populateModelDropdown(e.target.value);
  });
}
