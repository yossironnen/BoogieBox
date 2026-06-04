/**
 * Defines Vinyl Fx Engine behavior for BoogieBox.
 */

let ctx: AudioContext | null = null;
let decodedNeedleDrop: AudioBuffer | null = null;
let loadingPromise: Promise<void> | null = null;
let fallbackAudio: HTMLAudioElement | null = null;
const NEEDLE_DROP_URL = '/audio/vinyl-needle-drop.wav';

function canUseWebAudio(): boolean {
  return typeof window !== 'undefined' && typeof (window.AudioContext || (window as any).webkitAudioContext) === 'function';
}

function getAudioContext(): AudioContext | null {
  if (!canUseWebAudio()) return null;
  if (!ctx) {
    const Ctor = window.AudioContext || (window as any).webkitAudioContext;
    ctx = new Ctor();
  }
  return ctx;
}

async function loadNeedleDropWebAudio(): Promise<void> {
  const audioCtx = getAudioContext();
  if (!audioCtx || decodedNeedleDrop || loadingPromise) return loadingPromise ?? Promise.resolve();
  loadingPromise = fetch(NEEDLE_DROP_URL)
    .then((r) => {
      if (!r.ok) throw new Error(`needle-drop asset not found: ${r.status}`);
      return r.arrayBuffer();
    })
    .then((buf) => audioCtx.decodeAudioData(buf))
    .then((decoded) => {
      decodedNeedleDrop = decoded;
    })
    .catch(() => {
      decodedNeedleDrop = null;
    })
    .finally(() => {
      loadingPromise = null;
    });
  return loadingPromise;
}

function ensureFallbackAudio(): HTMLAudioElement | null {
  if (typeof window === 'undefined') return null;
  if (fallbackAudio) return fallbackAudio;
  const el = new Audio(NEEDLE_DROP_URL);
  el.preload = 'auto';
  fallbackAudio = el;
  return fallbackAudio;
}

/** Preload Vinyl Fx is part of this module's public API. */
export async function preloadVinylFx(): Promise<void> {
  if (canUseWebAudio()) {
    await loadNeedleDropWebAudio();
    return;
  }
  const el = ensureFallbackAudio();
  if (!el) return;
  try { await el.load(); } catch {}
}

/** Play Needle Drop is part of this module's public API. */
export async function playNeedleDrop(intensity: number): Promise<void> {
  const gain = Math.max(0, Math.min(1, intensity));
  if (gain <= 0) return;

  const audioCtx = getAudioContext();
  if (audioCtx) {
    if (audioCtx.state === 'suspended') {
      try { await audioCtx.resume(); } catch {}
    }
    if (!decodedNeedleDrop) {
      await loadNeedleDropWebAudio();
    }
    if (decodedNeedleDrop) {
      try {
        const source = audioCtx.createBufferSource();
        source.buffer = decodedNeedleDrop;
        const node = audioCtx.createGain();
        node.gain.value = gain;
        source.connect(node).connect(audioCtx.destination);
        source.start();
        return;
      } catch {
        // Fall through to <audio> fallback.
      }
    }
  }

  const fallback = ensureFallbackAudio();
  if (!fallback) return;
  try {
    fallback.currentTime = 0;
    fallback.volume = gain;
    await fallback.play();
  } catch {
    // Best-effort only.
  }
}

