/**
 * MowisAI Desktop — Speech Recognition
 */

import { $ } from './state.js';

export function initSpeechRecognition() {
  const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SR) {
    console.warn('Speech recognition not available');
    document.querySelectorAll('.btn-mic').forEach(b => b.style.display = 'none');
    return;
  }

  const recognition = new SR();
  recognition.continuous = true;
  recognition.interimResults = true;
  recognition.lang = 'en-US';

  let activeBtn = null;
  let activeTextarea = null;
  let isRecording = false;
  let finalTranscript = '';
  let silenceTimer = null;
  const SILENCE_TIMEOUT = 3000;
  let audioCtx = null;
  let analyser = null;
  let micStream = null;
  let animFrame = null;

  const waveformOverlay = $('waveform-overlay');
  const waveformCanvas = $('waveform-canvas');
  const waveformCancel = $('waveform-cancel');
  const waveformDone = $('waveform-done');

  function drawWaveform(canvas, analyserNode) {
    const ctx = canvas.getContext('2d');
    const W = canvas.width;
    const H = canvas.height;
    const data = new Uint8Array(analyserNode.frequencyBinCount);

    function frame() {
      if (!isRecording) {
        ctx.clearRect(0, 0, W, H);
        return;
      }
      animFrame = requestAnimationFrame(frame);
      analyserNode.getByteFrequencyData(data);

      ctx.clearRect(0, 0, W, H);
      
      const barCount = 80;
      const barW = 2;
      const gap = 4;
      const totalW = barCount * (barW + gap) - gap;
      const startX = (W - totalW) / 2;
      
      ctx.fillStyle = 'rgba(235, 235, 236, 0.7)';

      for (let i = 0; i < barCount; i++) {
        const idx = Math.floor((i + 1) * data.length / (barCount + 1));
        const val = data[idx] / 255;
        
        const barH = Math.max(2, val * (H * 0.85));
        const x = startX + i * (barW + gap);
        const y = (H - barH) / 2;
        
        ctx.fillRect(x, y, barW, barH);
      }
    }
    frame();
  }

  function drawBars(canvas, analyserNode) {
    const ctx = canvas.getContext('2d');
    const W = canvas.width;
    const H = canvas.height;
    const data = new Uint8Array(analyserNode.frequencyBinCount);

    function frame() {
      if (!isRecording) {
        ctx.clearRect(0, 0, W, H);
        return;
      }
      animFrame = requestAnimationFrame(frame);
      analyserNode.getByteFrequencyData(data);

      ctx.clearRect(0, 0, W, H);
      ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--blue').trim() || '#4a9eff';

      const barCount = 5;
      const barW = 2;
      const gap = 2;
      const totalW = barCount * barW + (barCount - 1) * gap;
      const startX = (W - totalW) / 2;

      for (let i = 0; i < barCount; i++) {
        const idx = Math.floor((i + 1) * data.length / (barCount + 1));
        const val = data[idx] / 255;
        const barH = Math.max(3, val * (H - 2));
        const x = startX + i * (barW + gap);
        const y = (H - barH) / 2;
        ctx.fillRect(x, y, barW, barH);
      }
    }
    frame();
  }

  async function startAudioViz(btn) {
    try {
      micStream = await navigator.mediaDevices.getUserMedia({ audio: true });
      audioCtx = new (window.AudioContext || window.webkitAudioContext)();
      const source = audioCtx.createMediaStreamSource(micStream);
      analyser = audioCtx.createAnalyser();
      analyser.fftSize = 128;
      source.connect(analyser);

      const canvas = btn.querySelector('.mic-bars');
      if (canvas) drawBars(canvas, analyser);
      
      if (waveformCanvas) {
        drawWaveform(waveformCanvas, analyser);
      }
    } catch (e) {
      console.warn('Mic access denied:', e);
    }
  }

  function stopAudioViz() {
    if (animFrame) cancelAnimationFrame(animFrame);
    animFrame = null;
    if (micStream) {
      micStream.getTracks().forEach(t => t.stop());
      micStream = null;
    }
    if (audioCtx) {
      audioCtx.close();
      audioCtx = null;
    }
    analyser = null;
    document.querySelectorAll('.mic-bars').forEach(c => {
      c.getContext('2d').clearRect(0, 0, c.width, c.height);
    });
    if (waveformCanvas) {
      waveformCanvas.getContext('2d').clearRect(0, 0, waveformCanvas.width, waveformCanvas.height);
    }
  }

  function showWaveformOverlay() {
    // Removed — keep only inline mic indicator
  }

  function hideWaveformOverlay() {
    // Removed — keep only inline mic indicator
  }

  recognition.onresult = (event) => {
    if (silenceTimer) clearTimeout(silenceTimer);
    silenceTimer = setTimeout(() => {
      if (isRecording) recognition.stop();
    }, SILENCE_TIMEOUT);

    let interim = '';
    for (let i = event.resultIndex; i < event.results.length; i++) {
      if (event.results[i].isFinal) {
        finalTranscript += event.results[i][0].transcript;
      } else {
        interim += event.results[i][0].transcript;
      }
    }
    if (activeTextarea) {
      const base = activeTextarea.dataset.preSpeech || '';
      activeTextarea.value = base + finalTranscript + interim;
    }
  };

  recognition.onend = () => {
    if (silenceTimer) { clearTimeout(silenceTimer); silenceTimer = null; }
    isRecording = false;
    stopAudioViz();
    if (activeBtn) activeBtn.classList.remove('recording');
    if (activeTextarea) {
      const base = activeTextarea.dataset.preSpeech || '';
      activeTextarea.value = base + finalTranscript;
      delete activeTextarea.dataset.preSpeech;
    }
    activeBtn = null;
    activeTextarea = null;
    finalTranscript = '';
  };

  recognition.onerror = (e) => {
    console.error('Speech recognition error:', e.error);
    isRecording = false;
    stopAudioViz();
    hideWaveformOverlay();
    if (activeBtn) activeBtn.classList.remove('recording');
    activeBtn = null;
    activeTextarea = null;
    finalTranscript = '';
  };

  async function toggleMic(btn, textarea) {
    if (isRecording) {
      recognition.stop();
    } else {
      activeBtn = btn;
      activeTextarea = textarea;
      finalTranscript = '';
      textarea.dataset.preSpeech = textarea.value;
      btn.classList.add('recording');
      isRecording = true;
      showWaveformOverlay();
      await startAudioViz(btn);
      recognition.start();
    }
  }

  if (waveformCancel) {
    waveformCancel.addEventListener('click', () => {
      if (isRecording) {
        recognition.stop();
        if (activeTextarea && activeTextarea.dataset.preSpeech !== undefined) {
          activeTextarea.value = activeTextarea.dataset.preSpeech;
        }
        finalTranscript = '';
      }
    });
  }

  if (waveformDone) {
    waveformDone.addEventListener('click', () => {
      if (isRecording) {
        recognition.stop();
      }
    });
  }

  $('btn-home-mic')?.addEventListener('click', () => toggleMic($('btn-home-mic'), $('home-input')));
  $('btn-chat-mic')?.addEventListener('click', () => toggleMic($('btn-chat-mic'), $('chat-input')));
}
