/**
 * MowisAI Desktop — File Attachments
 */

import { $, escHtml } from './state.js';

export const pendingAttachments = [];

function renderAttachments() {
  ['home-attachments', 'compose-attachments'].forEach(id => {
    const container = $(id);
    if (!container) return;
    if (pendingAttachments.length === 0) {
      container.classList.add('hidden');
      container.innerHTML = '';
      return;
    }
    container.classList.remove('hidden');
    container.innerHTML = pendingAttachments.map((f, i) => {
      if (f.isImage && f.dataUrl) {
        return `<div class="attach-chip attach-img">
          <img src="${f.dataUrl}" alt="${escHtml(f.name)}">
          <span class="attach-name">${escHtml(f.name)}</span>
          <button class="attach-remove" data-idx="${i}" title="Remove">&times;</button>
        </div>`;
      }
      return `<div class="attach-chip">
        <span class="material-symbols-outlined" style="font-size:14px">description</span>
        <span class="attach-name">${escHtml(f.name)}</span>
        <button class="attach-remove" data-idx="${i}" title="Remove">&times;</button>
      </div>`;
    }).join('');
    container.querySelectorAll('.attach-remove').forEach(el => {
      el.addEventListener('click', (e) => {
        e.stopPropagation();
        pendingAttachments.splice(Number(el.dataset.idx), 1);
        renderAttachments();
      });
    });
  });
}

function handleFileSelect(files) {
  for (const file of files) {
    const isImage = file.type.startsWith('image/');
    const entry = { name: file.name, type: file.type, size: file.size, isImage, dataUrl: null, base64: null };
    const reader = new FileReader();
    reader.onload = () => {
      if (isImage) {
        entry.dataUrl = reader.result;
        entry.base64 = reader.result.split(',')[1];
      } else {
        entry.base64 = btoa(reader.result);
      }
      renderAttachments();
    };
    if (isImage) {
      reader.readAsDataURL(file);
    } else {
      reader.readAsBinaryString(file);
    }
    pendingAttachments.push(entry);
  }
  renderAttachments();
}

export function buildAttachmentPayload() {
  const images = [];
  const fileNames = [];
  for (const f of pendingAttachments) {
    if (f.isImage && f.base64) {
      images.push({ data_url: `data:${f.type};base64,${f.base64}`, media_type: f.type, name: f.name });
      fileNames.push(`[image: ${f.name}]`);
    } else if (f.base64) {
      fileNames.push(`[file: ${f.name} (${f.type}, ${Math.round(f.size/1024)}KB)]`);
    }
  }
  return { fileRef: fileNames.join(' '), images };
}

export function clearAttachments() {
  pendingAttachments.length = 0;
  renderAttachments();
}

export function initFileUpload() {
  const homeInput = $('home-file-input');
  const chatInput = $('chat-file-input');

  $('btn-home-attach')?.addEventListener('click', () => homeInput?.click());
  $('btn-chat-attach')?.addEventListener('click', () => chatInput?.click());

  homeInput?.addEventListener('change', (e) => { handleFileSelect(e.target.files); e.target.value = ''; });
  chatInput?.addEventListener('change', (e) => { handleFileSelect(e.target.files); e.target.value = ''; });

  ['home-compose', 'compose-inner'].forEach(id => {
    const el = $(id);
    if (!el) return;
    el.addEventListener('dragover', (e) => { e.preventDefault(); el.style.borderColor = 'var(--blue)'; });
    el.addEventListener('dragleave', () => { el.style.borderColor = ''; });
    el.addEventListener('drop', (e) => {
      e.preventDefault();
      el.style.borderColor = '';
      if (e.dataTransfer.files.length) handleFileSelect(e.dataTransfer.files);
    });
  });
}
