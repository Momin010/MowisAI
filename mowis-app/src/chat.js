import { State, $, setText, escHtml, mdLite } from './state.js';
import { invoke } from './bridge.js';

export function appendChatMessage({ kind, content, ts }) {
  const container = $('chat-messages');
  if (!container) return;

  let row;

  if (kind === 'user') {
    row = createUserRow(content);
  } else if (kind === 'agent' || kind === 'assistant') {
    row = createAgentRow(content);
  } else if (kind === 'system') {
    row = createSystemRow(content);
  } else if (kind === 'error') {
    row = createErrorRow(content);
  }

  if (row) {
    container.appendChild(row);
    scrollToBottom(container);
  }
}

export function renderAgentMessageParts(parts) {
  const container = $('chat-messages');
  if (!container) return;

  for (const part of parts) {
    if (part.type === 'text' && part.text) {
      const row = createAgentRow(part.text);
      container.appendChild(row);
    } else if (part.type === 'reasoning' && part.text) {
      const row = document.createElement('div');
      row.className = 'msg-row agent';
      const block = document.createElement('div');
      block.className = 'reasoning-block';
      block.textContent = part.text;
      row.appendChild(block);
      container.appendChild(row);
    } else if (part.type === 'tool_call') {
      renderToolCall(part);
    } else if (part.type === 'tool_result') {
      renderToolResult(part);
    } else if (part.type === 'finish') {
      // Handled by caller
    }
  }

  scrollToBottom(container);
}

function createUserRow(content) {
  const row = document.createElement('div');
  row.className = 'msg-row user';
  const bubble = document.createElement('div');
  bubble.className = 'msg-bubble';
  bubble.textContent = content;
  row.appendChild(bubble);
  return row;
}

function createAgentRow(content) {
  const row = document.createElement('div');
  row.className = 'msg-row agent';
  const bubble = document.createElement('div');
  bubble.className = 'msg-bubble';
  bubble.innerHTML = mdLite(content);
  row.appendChild(bubble);
  return row;
}

function createSystemRow(content) {
  const row = document.createElement('div');
  row.className = 'msg-row system';
  const bubble = document.createElement('div');
  bubble.className = 'msg-bubble';
  bubble.textContent = content;
  row.appendChild(bubble);
  return row;
}

function createErrorRow(content) {
  const row = document.createElement('div');
  row.className = 'msg-row system';
  row.style.padding = '4px 40px';
  const card = document.createElement('div');
  card.className = 'error-card';
  card.textContent = content;
  row.appendChild(card);
  return row;
}

export function renderToolCall(part) {
  const container = $('chat-messages');
  if (!container) return;

  const row = document.createElement('div');
  row.className = 'msg-row tool-call';

  const card = document.createElement('div');
  card.className = 'tool-call-card';

  const icon = getToolIcon(part.name);
  const inputPreview = formatToolInput(part.name, part.input);

  const header = document.createElement('div');
  header.className = 'tool-call-header';

  const iconEl = document.createElement('span');
  iconEl.className = 'tool-call-icon';
  iconEl.innerHTML = icon;

  const nameEl = document.createElement('span');
  nameEl.className = 'tool-call-name';
  nameEl.textContent = part.name || 'tool';

  const statusEl = document.createElement('span');
  statusEl.className = 'tool-call-status running';
  statusEl.textContent = 'running';

  header.appendChild(iconEl);
  header.appendChild(nameEl);
  header.appendChild(statusEl);
  card.appendChild(header);

  if (inputPreview) {
    const inputEl = document.createElement('div');
    inputEl.className = 'tool-call-input';
    inputEl.innerHTML = inputPreview;
    card.appendChild(inputEl);
  }

  const resultEl = document.createElement('div');
  resultEl.className = 'tool-call-result';
  card.appendChild(resultEl);

  card.addEventListener('click', () => {
    resultEl.classList.toggle('visible');
  });

  row.appendChild(card);
  container.appendChild(row);
  scrollToBottom(container);
}

export function renderToolResult(part) {
  const container = $('chat-messages');
  if (!container) return;

  const toolCards = container.querySelectorAll('.tool-call-card');
  const lastCard = toolCards[toolCards.length - 1];
  if (!lastCard) return;

  const statusEl = lastCard.querySelector('.tool-call-status');
  if (statusEl) {
    statusEl.className = `tool-call-status ${part.is_error ? 'error' : 'done'}`;
    statusEl.textContent = part.is_error ? 'error' : 'done';
  }

  const resultEl = lastCard.querySelector('.tool-call-result');
  if (resultEl && part.content) {
    const preview = part.content.length > 800 ? part.content.slice(0, 800) + '...' : part.content;
    resultEl.textContent = preview;
    if (part.is_error) resultEl.classList.add('error');
  }

  scrollToBottom(container);
}

function getToolIcon(name) {
  const icons = {
    bash: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>',
    edit: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path></svg>',
    write: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline></svg>',
    read: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><path d="M16 13H8"></path><path d="M16 17H8"></path></svg>',
    glob: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
    grep: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
  };
  const n = (name || '').toLowerCase();
  for (const [key, icon] of Object.entries(icons)) {
    if (n.includes(key)) return icon;
  }
  return icons.bash;
}

function formatToolInput(name, input) {
  if (!input) return '';
  const n = (name || '').toLowerCase();
  if (n.includes('bash') && input.command) return `<code>${escHtml(input.command)}</code>`;
  if ((n.includes('edit') || n.includes('write')) && input.path) return `<code>${escHtml(input.path)}</code>`;
  if (n.includes('read') && input.path) return `<code>${escHtml(input.path)}</code>`;
  if (n.includes('glob') && input.pattern) return `<code>${escHtml(input.pattern)}</code>`;
  if (n.includes('grep') && input.pattern) return `<code>${escHtml(input.pattern)}</code>`;
  if (input.path) return `<code>${escHtml(input.path)}</code>`;
  if (input.command) return `<code>${escHtml(input.command)}</code>`;
  return '';
}

export function appendThinkingIndicator() {
  const container = $('chat-messages');
  if (!container) return;
  const row = document.createElement('div');
  row.className = 'msg-row agent';
  row.id = 'thinking-indicator';
  const bubble = document.createElement('div');
  bubble.className = 'msg-bubble thinking';
  bubble.innerHTML = '<span class="thinking-dots"><span>.</span><span>.</span><span>.</span></span>';
  row.appendChild(bubble);
  container.appendChild(row);
  scrollToBottom(container);
}

export function removeThinkingIndicator() {
  $('thinking-indicator')?.remove();
}

export function scrollToBottom(el) {
  if (!el) el = $('chat-messages');
  if (el) requestAnimationFrame(() => { el.scrollTop = el.scrollHeight; });
}
