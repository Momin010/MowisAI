/**
 * MowisAI Desktop — Chat Rendering & Agent Polling
 */

import { State, $, setText, escHtml, mdLite } from './state.js';
import { invoke } from './bridge.js';
import { nowTs } from './utils.js';

// ── External callbacks (set from main.js to avoid circular imports) ──────────

let _setTaskPanelOpen = null;
let _renderTaskPanel = null;
let _setSessionActive = null;

export function setChatCallbacks({ setTaskPanelOpen, renderTaskPanel, setSessionActive }) {
  _setTaskPanelOpen = setTaskPanelOpen;
  _renderTaskPanel = renderTaskPanel;
  _setSessionActive = setSessionActive;
}

// ── Chat rendering ───────────────────────────────────────────────────────────

export function appendChatMessage(msg) {
  const container = $('chat-messages');
  if (!container) return;

  if (State.isStreaming && msg.kind !== 'agent_chunk') {
    finalizeStreaming();
  }

  let row;

  if (msg.kind === 'user') {
    row = createMessageRow('user', msg.content);
  } else if (msg.kind === 'agent') {
    row = createMessageRow('agent', msg.content);
    if (msg.streaming) State.isStreaming = true;
  } else if (msg.kind === 'system') {
    row = createMessageRow('system', msg.content);
  } else if (msg.kind === 'plan') {
    if (!State.planCardShown) {
      State.planCardShown = true;
      row = createPlanCard(msg);
    }
  } else if (msg.kind === 'error') {
    row = createErrorCard(msg.content);
  }

  if (row) {
    container.appendChild(row);
    scrollToBottom(container);
  }
}

export function appendAgentChunk(chunk) {
  const container = $('chat-messages');
  if (!container) return;

  if (!State.isStreaming) {
    State.streamingContent = '';
    State.isStreaming = true;
    const row = document.createElement('div');
    row.className = 'msg-row agent';
    row.id = 'streaming-bubble';
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    bubble.id = 'streaming-text';
    const cursor = document.createElement('span');
    cursor.className = 'cursor';
    cursor.id = 'streaming-cursor';
    bubble.appendChild(cursor);
    row.appendChild(bubble);
    container.appendChild(row);
  }

  State.streamingContent += chunk;

  const textEl = $('streaming-text');
  if (textEl) {
    const html = mdLite(State.streamingContent);
    textEl.innerHTML = html;
    const cur = document.createElement('span');
    cur.className = 'cursor';
    cur.id = 'streaming-cursor';
    textEl.appendChild(cur);
  }

  scrollToBottom(container);
}

export function finalizeStreaming() {
  if (!State.isStreaming) return;
  State.isStreaming = false;
  const cursor = $('streaming-cursor');
  if (cursor) cursor.remove();
  const bubble = $('streaming-bubble');
  if (bubble) bubble.removeAttribute('id');
  const text = $('streaming-text');
  if (text) text.removeAttribute('id');
  State.streamingContent = '';
}

export function appendFileChanges(changes) {
  const container = $('chat-messages');
  if (!container) return;

  if (State.isStreaming) {
    finalizeStreaming();
  }

  const row = document.createElement('div');
  row.className = 'msg-row file-changes';
  
  const card = document.createElement('div');
  card.className = 'file-changes-card';
  
  changes.forEach(change => {
    const item = document.createElement('div');
    item.className = 'file-change-item';
    item.dataset.action = change.action;
    item.title = change.path;
    
    const icon = document.createElement('span');
    icon.className = 'file-icon';
    icon.innerHTML = getFileActionIcon(change.action);
    
    const filename = change.path.split('/').pop() || change.path;
    const label = document.createElement('span');
    label.className = 'file-label';
    label.textContent = filename;
    
    if (change.lines_added > 0 || change.lines_deleted > 0) {
      const badge = document.createElement('span');
      badge.className = 'line-count-badge';
      const parts = [];
      if (change.lines_added > 0) parts.push(`+${change.lines_added}`);
      if (change.lines_deleted > 0) parts.push(`-${change.lines_deleted}`);
      badge.textContent = parts.join(' ');
      label.appendChild(badge);
    }
    
    item.appendChild(icon);
    item.appendChild(label);
    
    item.addEventListener('click', () => {
      openDiffViewer(change);
    });
    
    card.appendChild(item);
  });
  
  row.appendChild(card);
  container.appendChild(row);
  scrollToBottom(container);
}

export function createMessageRow(type, content) {
  const row = document.createElement('div');
  row.className = `msg-row ${type}`;
  const bubble = document.createElement('div');
  bubble.className = 'msg-bubble';
  bubble.innerHTML = type === 'agent' ? mdLite(content) : escHtml(content);
  row.appendChild(bubble);
  return row;
}

export function createPlanCard(msg) {
  const row = document.createElement('div');
  row.className = 'msg-row plan';
  row.style.padding = '0 40px';

  const card = document.createElement('div');
  card.className = 'plan-card';
  card.innerHTML = `
    <div class="plan-card-title">▶ Orchestration Plan</div>
    <div class="plan-card-row">
      <div class="plan-stat">
        <div class="plan-stat-val">${msg.task_count}</div>
        <div class="plan-stat-lbl">Tasks</div>
      </div>
      <div class="plan-stat">
        <div class="plan-stat-val">${msg.agent_count}</div>
        <div class="plan-stat-lbl">Agents</div>
      </div>
      <div class="plan-stat">
        <div class="plan-stat-val">${(msg.mode || 'auto').toUpperCase()}</div>
        <div class="plan-stat-lbl">Mode</div>
      </div>
    </div>
    <div class="plan-sandboxes">
      ${(msg.sandboxes || []).map(s => `<span class="plan-sb${s === 'zero' ? ' plan-sb-zero' : ''}">${s === 'zero' ? '✦ ' : ''}${s}</span>`).join('')}
    </div>
  `;
  card.addEventListener('click', () => { if (_setTaskPanelOpen) _setTaskPanelOpen(true); });
  row.appendChild(card);
  return row;
}

export function createErrorCard(content) {
  const row = document.createElement('div');
  row.className = 'msg-row system';
  row.style.padding = '4px 40px';
  const card = document.createElement('div');
  card.className = 'error-card';
  card.textContent = content;
  row.appendChild(card);
  return row;
}

export function scrollToBottom(el) {
  requestAnimationFrame(() => { el.scrollTop = el.scrollHeight; });
}

// ── Thinking indicator ───────────────────────────────────────────────────────

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

// ── Agent polling ────────────────────────────────────────────────────────────

export function startAgentPolling(sessionId) {
  stopAgentPolling();
  State.lastMessageCount = 0;

  async function poll() {
    if (!State.agentSessionId || State.agentSessionId !== sessionId) return;
    try {
      const messages = await invoke('agent_list_messages', { sessionId });
      if (!messages || !Array.isArray(messages)) return;

      if (messages.length > State.lastMessageCount) {
        const newMessages = messages.slice(State.lastMessageCount);
        State.lastMessageCount = messages.length;

        removeThinkingIndicator();

        for (const msg of newMessages) {
          renderAgentMessage(msg);
        }

        const lastMsg = messages[messages.length - 1];
        if (lastMsg?.role === 'assistant') {
          const hasFinish = (lastMsg.parts || []).some(p => p.type === 'finish');
          if (hasFinish) {
            finalizeStreaming();
            if (_setSessionActive) _setSessionActive(false, true);
            stopAgentPolling();
            return;
          }
        }
      }
    } catch (e) {
      console.warn('Agent poll error:', e);
    }

    if (State.agentSessionId === sessionId) {
      State.pollTimer = setTimeout(poll, 1000);
    }
  }

  State.pollTimer = setTimeout(poll, 500);
}

export function stopAgentPolling() {
  if (State.pollTimer) {
    clearTimeout(State.pollTimer);
    State.pollTimer = null;
  }
}

export function renderAgentMessage(msg) {
  if (!msg) return;

  if (msg.role === 'user') {
    return;
  }

  if (msg.role === 'assistant') {
    const parts = msg.parts || [];
    for (const part of parts) {
      if (part.type === 'text' && part.text) {
        appendChatMessage({ kind: 'agent', content: part.text, streaming: false, ts: nowTs() });
      } else if (part.type === 'tool_call') {
        renderToolCall(part);
      } else if (part.type === 'tool_result') {
        renderToolResult(part);
      }
    }
  }
}

export function renderToolCall(part) {
  const container = $('chat-messages');
  if (!container) return;

  if (State.isStreaming) finalizeStreaming();

  const row = document.createElement('div');
  row.className = 'msg-row tool-call';

  const card = document.createElement('div');
  card.className = 'tool-call-card';

  const icon = getToolIcon(part.name);
  const inputPreview = formatToolInput(part.name, part.input);

  card.innerHTML = `
    <div class="tool-call-header">
      <span class="tool-call-icon">${icon}</span>
      <span class="tool-call-name">${escHtml(part.name)}</span>
      <span class="tool-call-status running">running</span>
    </div>
    ${inputPreview ? `<div class="tool-call-input">${inputPreview}</div>` : ''}
  `;

  row.appendChild(card);
  container.appendChild(row);
  scrollToBottom(container);
}

export function renderToolResult(part) {
  const container = $('chat-messages');
  if (!container) return;

  const toolCards = container.querySelectorAll('.tool-call-card');
  const lastCard = toolCards[toolCards.length - 1];
  if (lastCard) {
    const statusEl = lastCard.querySelector('.tool-call-status');
    if (statusEl) {
      statusEl.className = `tool-call-status ${part.is_error ? 'error' : 'done'}`;
      statusEl.textContent = part.is_error ? 'error' : 'done';
    }

    if (part.content) {
      const resultEl = document.createElement('div');
      resultEl.className = `tool-call-result ${part.is_error ? 'error' : ''}`;
      const preview = part.content.length > 500 ? part.content.slice(0, 500) + '…' : part.content;
      resultEl.textContent = preview;
      lastCard.appendChild(resultEl);
    }
  }

  scrollToBottom(container);
}

export function getToolIcon(name) {
  const icons = {
    bash: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>',
    edit: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path></svg>',
    write: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline></svg>',
    read: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><path d="M16 13H8"></path><path d="M16 17H8"></path></svg>',
    glob: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
    grep: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
  };
  return icons[name] || icons.bash;
}

export function formatToolInput(name, input) {
  if (!input) return '';
  if (name === 'bash' && input.command) {
    return `<code>${escHtml(input.command)}</code>`;
  }
  if ((name === 'edit' || name === 'write') && input.path) {
    return `<code>${escHtml(input.path)}</code>`;
  }
  if (name === 'read' && input.path) {
    return `<code>${escHtml(input.path)}</code>`;
  }
  if (name === 'glob' && input.pattern) {
    return `<code>${escHtml(input.pattern)}</code>`;
  }
  if (name === 'grep' && input.pattern) {
    return `<code>${escHtml(input.pattern)}</code>`;
  }
  return '';
}

// ── Diff Viewer ──────────────────────────────────────────────────────────────

export function openDiffViewer(change) {
  const overlay = document.createElement('div');
  overlay.className = 'diff-viewer-overlay';
  
  const modal = document.createElement('div');
  modal.className = 'diff-viewer-modal';
  
  const header = document.createElement('div');
  header.className = 'diff-viewer-header';
  
  const title = document.createElement('div');
  title.className = 'diff-viewer-title';
  title.textContent = change.path;
  
  const stats = document.createElement('div');
  stats.className = 'diff-viewer-stats';
  if (change.lines_added > 0 || change.lines_deleted > 0) {
    const parts = [];
    if (change.lines_added > 0) parts.push(`+${change.lines_added} added`);
    if (change.lines_deleted > 0) parts.push(`-${change.lines_deleted} deleted`);
    stats.textContent = parts.join(' • ');
  } else {
    stats.textContent = change.action;
  }
  
  const closeBtn = document.createElement('button');
  closeBtn.className = 'diff-viewer-close';
  closeBtn.innerHTML = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>`;
  closeBtn.onclick = () => overlay.remove();
  
  header.appendChild(title);
  header.appendChild(stats);
  header.appendChild(closeBtn);
  
  const content = document.createElement('div');
  content.className = 'diff-viewer-content';
  
  if (change.content) {
    const lines = change.content.split('\n');
    const codeBlock = document.createElement('pre');
    codeBlock.className = 'diff-viewer-code';
    
    lines.forEach((line, idx) => {
      const lineDiv = document.createElement('div');
      lineDiv.className = 'code-line';
      
      const lineNum = document.createElement('span');
      lineNum.className = 'line-number';
      lineNum.textContent = idx + 1;
      
      const lineContent = document.createElement('span');
      lineContent.className = 'line-content';
      lineContent.textContent = line || ' ';
      
      lineDiv.appendChild(lineNum);
      lineDiv.appendChild(lineContent);
      codeBlock.appendChild(lineDiv);
    });
    
    content.appendChild(codeBlock);
  } else {
    const placeholder = document.createElement('div');
    placeholder.className = 'diff-viewer-placeholder';
    placeholder.textContent = change.action === 'deleted' 
      ? 'File was deleted' 
      : 'Content not available';
    content.appendChild(placeholder);
  }
  
  modal.appendChild(header);
  modal.appendChild(content);
  overlay.appendChild(modal);
  document.body.appendChild(overlay);
  
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) overlay.remove();
  });
  
  const escHandler = (e) => {
    if (e.key === 'Escape') {
      overlay.remove();
      document.removeEventListener('keydown', escHandler);
    }
  };
  document.addEventListener('keydown', escHandler);
}

// ── Diff Panel (right sidebar) ───────────────────────────────────────────────

function normalizeNewlines(text) {
  if (text == null) return '';
  let t = String(text);
  if (t.includes('\\n') && !t.includes('\n')) t = t.replace(/\\n/g, '\n');
  return t.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
}

function diffLines(beforeText, afterText) {
  const a = normalizeNewlines(beforeText).split('\n');
  const b = normalizeNewlines(afterText).split('\n');

  const N = a.length;
  const M = b.length;
  const max = N + M;
  const v = new Map();
  v.set(1, 0);
  const trace = [];

  for (let d = 0; d <= max; d++) {
    const vNext = new Map(v);
    for (let k = -d; k <= d; k += 2) {
      let x;
      if (k === -d || (k !== d && (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0))) {
        x = v.get(k + 1) ?? 0;
      } else {
        x = (v.get(k - 1) ?? 0) + 1;
      }
      let y = x - k;
      while (x < N && y < M && a[x] === b[y]) {
        x++;
        y++;
      }
      vNext.set(k, x);
      if (x >= N && y >= M) {
        trace.push(vNext);
        return backtrack(trace, a, b);
      }
    }
    trace.push(vNext);
    v.clear();
    for (const [k, x] of vNext) v.set(k, x);
  }

  return b.map(line => ({ type: 'ctx', text: line }));
}

function backtrack(trace, a, b) {
  let x = a.length;
  let y = b.length;
  const edits = [];

  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d];
    const k = x - y;
    let prevK;
    if (k === -d || (k !== d && (v.get(k - 1) ?? 0) < (v.get(k + 1) ?? 0))) {
      prevK = k + 1;
    } else {
      prevK = k - 1;
    }
    const prevX = v.get(prevK) ?? 0;
    const prevY = prevX - prevK;

    while (x > prevX && y > prevY) {
      edits.push({ type: 'ctx', text: a[x - 1] });
      x--;
      y--;
    }

    if (d === 0) break;

    if (x === prevX) {
      edits.push({ type: 'add', text: b[y - 1] });
      y--;
    } else {
      edits.push({ type: 'del', text: a[x - 1] });
      x--;
    }
  }

  edits.reverse();
  return edits;
}

function latestFlattenedChanges() {
  const flat = [];
  for (const batch of State.fileChanges) {
    for (const c of batch.changes) flat.push(c);
  }
  const seen = new Set();
  const out = [];
  for (const c of flat) {
    if (seen.has(c.path)) continue;
    seen.add(c.path);
    out.push(c);
  }
  return out;
}

function buildChangeTree(changes) {
  const root = { name: '', path: '', kind: 'dir', children: new Map() };
  for (const c of changes) {
    const parts = String(c.path || '').split('/').filter(Boolean);
    let cur = root;
    let curPath = '';
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      curPath = curPath ? `${curPath}/${part}` : part;
      const isLeaf = i === parts.length - 1;
      if (isLeaf) {
        cur.children.set(curPath, { name: part, path: curPath, kind: 'file', change: c });
      } else {
        if (!cur.children.has(curPath)) {
          cur.children.set(curPath, { name: part, path: curPath, kind: 'dir', children: new Map() });
        }
        cur = cur.children.get(curPath);
      }
    }
  }
  return root;
}

function treeToRows(node, depth, rows, opts) {
  const { query, actions, expanded } = opts;
  const kids = [...(node.children?.values() || [])];
  kids.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'dir' ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  for (const child of kids) {
    if (child.kind === 'file') {
      const c = child.change;
      const okAction = actions.has(String(c.action));
      const okQuery = !query || child.path.toLowerCase().includes(query);
      if (!okAction || !okQuery) continue;
      rows.push({ kind: 'file', depth, path: c.path, name: child.name, action: c.action });
      continue;
    }

    const isOpen = expanded.has(child.path);
    const tmp = [];
    treeToRows(child, depth + 1, tmp, opts);
    if (tmp.length === 0) continue;

    rows.push({ kind: 'dir', depth, path: child.path, name: child.name, open: isOpen, count: tmp.filter(r => r.kind === 'file').length });
    if (isOpen) rows.push(...tmp);
  }
}

export function renderDiffPanel() {
  const panel = $('task-panel');
  if (!panel) return;

  const title = panel.querySelector('.task-panel-title');
  if (title) title.textContent = 'Diff';
  const subtitle = $('task-panel-subtitle');
  if (subtitle) subtitle.textContent = 'Select a file to inspect changes';

  const body = $('task-panel-body');
  const detail = $('task-detail');
  if (!body || !detail) return;

  const changes = latestFlattenedChanges();
  const tree = buildChangeTree(changes);
  const q = (State.diffTree?.query || '').trim().toLowerCase();
  const actions = State.diffTree?.actions || new Set(['created', 'modified', 'deleted', 'moved', 'read']);
  const expanded = State.diffTree?.expanded || new Set();
  const rows = [];
  treeToRows(tree, 0, rows, { query: q, actions, expanded });

  const actionBtn = (id, label) => {
    const on = actions.has(id);
    return `<button class="diff-filter ${on ? 'on' : ''}" data-action="${escHtml(id)}">${escHtml(label)}</button>`;
  };

  const controls = `
    <div class="diff-tree-controls">
      <input class="diff-tree-search" id="diff-tree-search" placeholder="Search files…" value="${escHtml(State.diffTree?.query || '')}" />
      <div class="diff-filter-row">
        ${actionBtn('created','Created')}
        ${actionBtn('modified','Modified')}
        ${actionBtn('deleted','Deleted')}
        ${actionBtn('moved','Moved')}
        ${actionBtn('read','Read')}
        <button class="diff-filter ghost" id="diff-filter-all">All</button>
        <button class="diff-filter ghost" id="diff-filter-none">None</button>
        <button class="diff-filter ghost" id="diff-collapse-all">Collapse</button>
      </div>
    </div>
  `;

  const rowsHtml = changes.length === 0
    ? `<div class="task-row"><div class="task-desc">No file changes yet</div></div>`
    : rows.map(r => {
        const pad = `style="padding-left:${12 + r.depth * 14}px"`;
        if (r.kind === 'dir') {
          return `
            <div class="diff-tree-row dir" data-dir="${escHtml(r.path)}" ${pad}>
              <span class="chev">${r.open ? '▾' : '▸'}</span>
              <span class="name">${escHtml(r.name)}</span>
              <span class="meta">${r.count}</span>
            </div>
          `;
        }
        const selected = State.selectedChangePath === r.path ? 'selected' : '';
        return `
          <div class="diff-tree-row file ${selected}" data-path="${escHtml(r.path)}" ${pad}>
            <span class="task-dot ${r.action}"></span>
            <span class="name">${escHtml(r.name)}</span>
            <span class="meta">${escHtml(r.action)}</span>
          </div>
        `;
      }).join('');

  body.innerHTML = controls + `<div class="diff-tree-rows">${rowsHtml}</div>`;

  $('diff-tree-search')?.addEventListener('input', (e) => {
    State.diffTree.query = e.target.value || '';
    renderDiffPanel();
  });
  body.querySelectorAll('.diff-filter[data-action]').forEach(btn => {
    btn.addEventListener('click', () => {
      const a = btn.dataset.action;
      if (!a) return;
      if (State.diffTree.actions.has(a)) State.diffTree.actions.delete(a);
      else State.diffTree.actions.add(a);
      renderDiffPanel();
    });
  });
  $('diff-filter-all')?.addEventListener('click', () => {
    State.diffTree.actions = new Set(['created', 'modified', 'deleted', 'moved', 'read']);
    renderDiffPanel();
  });
  $('diff-filter-none')?.addEventListener('click', () => {
    State.diffTree.actions = new Set();
    renderDiffPanel();
  });
  $('diff-collapse-all')?.addEventListener('click', () => {
    State.diffTree.expanded = new Set();
    renderDiffPanel();
  });

  body.querySelectorAll('.diff-tree-row.dir').forEach(row => {
    row.addEventListener('click', () => {
      const d = row.dataset.dir;
      if (!d) return;
      if (State.diffTree.expanded.has(d)) State.diffTree.expanded.delete(d);
      else State.diffTree.expanded.add(d);
      renderDiffPanel();
    });
  });
  body.querySelectorAll('.diff-tree-row.file').forEach(row => {
    row.addEventListener('click', () => {
      const p = row.dataset.path;
      if (!p) return;
      State.selectedChangePath = p;
      renderDiffPanel();
    });
  });

  const selected = changes.find(c => c.path === State.selectedChangePath) || changes[0] || null;
  if (!State.selectedChangePath && selected) State.selectedChangePath = selected.path;

  if (!selected) {
    detail.innerHTML = `<div class="task-detail-empty">No diff to show yet.</div>`;
    return;
  }

  const before = selected.before_content ?? '';
  const after = selected.content ?? '';

  let hunks = [];
  if (selected.action === 'created') {
    hunks = normalizeNewlines(after).split('\n').map(t => ({ type: 'add', text: t }));
  } else if (selected.action === 'deleted') {
    hunks = normalizeNewlines(before).split('\n').map(t => ({ type: 'del', text: t }));
  } else if (selected.action === 'modified') {
    hunks = diffLines(before, after);
  } else {
    hunks = normalizeNewlines(after || before).split('\n').map(t => ({ type: 'ctx', text: t }));
  }

  const headerHtml = `
    <div class="diff-panel-head">
      <div class="diff-panel-path">${escHtml(selected.path)}</div>
      <div class="diff-panel-meta">${escHtml(selected.action)}</div>
    </div>
  `;

  let addCount = 0, delCount = 0;
  for (const h of hunks) { if (h.type === 'add') addCount++; if (h.type === 'del') delCount++; }
  if (subtitle) subtitle.textContent = `${addCount} added · ${delCount} removed`;

  const linesHtml = hunks.map((h, i) => {
    const sign = h.type === 'add' ? '+' : h.type === 'del' ? '-' : ' ';
    return `
      <div class="diff-line ${h.type}">
        <span class="diff-gutter">${sign}</span>
        <span class="diff-lno">${i + 1}</span>
        <span class="diff-text">${escHtml(h.text || ' ')}</span>
      </div>
    `;
  }).join('');

  detail.innerHTML = `${headerHtml}<div class="diff-panel-body">${linesHtml || '<div class="task-detail-empty">Empty file</div>'}</div>`;
}

function getFileActionIcon(action) {
  const icons = {
    created: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="12" y1="18" x2="12" y2="12"></line><line x1="9" y1="15" x2="15" y2="15"></line></svg>`,
    modified: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><path d="M10.4 12.6a2 2 0 1 1 3 3L8 21l-4 1 1-4Z"></path></svg>`,
    deleted: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="9" y1="15" x2="15" y2="15"></line></svg>`,
    read: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><path d="M16 13H8"></path><path d="M16 17H8"></path><path d="M10 9H8"></path></svg>`,
    moved: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><polyline points="10 9 9 9 8 9"></polyline><path d="m15 14-3-3 3-3"></path></svg>`,
  };
  return icons[action] || icons.modified;
}
