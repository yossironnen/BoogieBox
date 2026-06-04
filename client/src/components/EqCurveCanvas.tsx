/**
 * Defines the Eq Curve Canvas React component and related UI helpers.
 */

import React, { useRef, useEffect, useCallback } from 'react';
import {
  PARAMETRIC_DB_MIN, PARAMETRIC_DB_MAX, PARAMETRIC_FREQ_MIN, PARAMETRIC_FREQ_MAX,
  computeCombinedResponseDb, clampDb, clampFreq,
  type ParametricEqBand,
} from '../audio/eq';

// Frequency grid lines shown on the canvas (Hz)
const FREQ_GRID = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];
const FREQ_LABELS: Record<number, string> = {
  20: '20', 50: '50', 100: '100', 200: '200', 500: '500',
  1000: '1k', 2000: '2k', 5000: '5k', 10000: '10k', 20000: '20k',
};
const DB_GRID = [PARAMETRIC_DB_MAX, 6, 0, -6, PARAMETRIC_DB_MIN];
const CURVE_POINTS = 300; // number of x-samples for the response curve

const NODE_RADIUS = 7;
const MARGIN = { top: 8, right: 8, bottom: 20, left: 28 };

function resolveCssColor(value: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback;
  const trimmed = value.trim();
  const varMatch = /^var\((--[^),\s]+)/.exec(trimmed);
  if (!varMatch) return trimmed || fallback;
  return getComputedStyle(document.documentElement).getPropertyValue(varMatch[1]).trim() || fallback;
}

function freqToX(freq: number, width: number): number {
  const logMin = Math.log10(PARAMETRIC_FREQ_MIN);
  const logMax = Math.log10(PARAMETRIC_FREQ_MAX);
  return MARGIN.left + ((Math.log10(Math.max(PARAMETRIC_FREQ_MIN, freq)) - logMin) / (logMax - logMin)) * (width - MARGIN.left - MARGIN.right);
}

function xToFreq(x: number, width: number): number {
  const logMin = Math.log10(PARAMETRIC_FREQ_MIN);
  const logMax = Math.log10(PARAMETRIC_FREQ_MAX);
  const t = (x - MARGIN.left) / (width - MARGIN.left - MARGIN.right);
  return Math.pow(10, logMin + t * (logMax - logMin));
}

function dbToY(db: number, height: number): number {
  const range = PARAMETRIC_DB_MAX - PARAMETRIC_DB_MIN;
  return MARGIN.top + ((PARAMETRIC_DB_MAX - db) / range) * (height - MARGIN.top - MARGIN.bottom);
}

function yToDb(y: number, height: number): number {
  const range = PARAMETRIC_DB_MAX - PARAMETRIC_DB_MIN;
  return PARAMETRIC_DB_MAX - ((y - MARGIN.top) / (height - MARGIN.top - MARGIN.bottom)) * range;
}

function buildFrequencyArray(): Float32Array {
  const arr = new Float32Array(CURVE_POINTS);
  const logMin = Math.log10(PARAMETRIC_FREQ_MIN);
  const logMax = Math.log10(PARAMETRIC_FREQ_MAX);
  for (let i = 0; i < CURVE_POINTS; i++) {
    arr[i] = Math.pow(10, logMin + (i / (CURVE_POINTS - 1)) * (logMax - logMin));
  }
  return arr;
}

const FREQ_ARRAY = buildFrequencyArray();

/** Eq Curve Canvas Props is part of this module's public API. */
export interface EqCurveCanvasProps {
  bands: ParametricEqBand[];
  selectedBandIndex: number;
  accentColor: string;
  onChange: (index: number, updates: Partial<Pick<ParametricEqBand, 'frequencyHz' | 'gainDb'>>) => void;
  onSelectBand: (index: number) => void;
  width?: number;
  height?: number;
}

/** Eq Curve Canvas is part of this module's public API. */
export default function EqCurveCanvas({
  bands,
  selectedBandIndex,
  accentColor,
  onChange,
  onSelectBand,
  width = 536,
  height = 160,
}: EqCurveCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const dragRef = useRef<{ bandIndex: number; startX: number; startY: number; startFreq: number; startGain: number } | null>(null);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.width / dpr;
    const h = canvas.height / dpr;
    const accent = resolveCssColor(accentColor, '#7aa2ff');
    const surface = resolveCssColor('var(--surface)', '#171717');
    const border = resolveCssColor('var(--border)', '#3a3a3a');
    const text = resolveCssColor('var(--text)', '#ffffff');
    const muted = resolveCssColor('var(--text-muted)', '#a8a8a8');

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.save();
    ctx.scale(dpr, dpr);

    // Background
    ctx.globalAlpha = 0.42;
    ctx.fillStyle = surface;
    ctx.fillRect(MARGIN.left, MARGIN.top, w - MARGIN.left - MARGIN.right, h - MARGIN.top - MARGIN.bottom);
    ctx.globalAlpha = 1;

    // Grid lines — frequency (vertical)
    ctx.strokeStyle = border;
    ctx.lineWidth = 1;
    ctx.font = '9px system-ui, sans-serif';
    ctx.fillStyle = muted;
    ctx.textAlign = 'center';
    for (const freq of FREQ_GRID) {
      const x = freqToX(freq, w);
      ctx.globalAlpha = 0.55;
      ctx.beginPath();
      ctx.moveTo(x, MARGIN.top);
      ctx.lineTo(x, h - MARGIN.bottom);
      ctx.stroke();
      ctx.globalAlpha = 0.78;
      ctx.fillText(FREQ_LABELS[freq] ?? String(freq), x, h - 3);
    }
    ctx.globalAlpha = 1;

    // Grid lines — dB (horizontal)
    ctx.textAlign = 'right';
    for (const db of DB_GRID) {
      const y = dbToY(db, h);
      ctx.strokeStyle = db === 0 ? text : border;
      ctx.lineWidth = db === 0 ? 1.5 : 1;
      ctx.globalAlpha = db === 0 ? 0.28 : 0.55;
      ctx.beginPath();
      ctx.moveTo(MARGIN.left, y);
      ctx.lineTo(w - MARGIN.right, y);
      ctx.stroke();
      ctx.globalAlpha = 0.78;
      ctx.fillStyle = muted;
      ctx.fillText(db > 0 ? `+${db}` : String(db), MARGIN.left - 3, y + 3);
    }
    ctx.globalAlpha = 1;

    // Frequency response curve
    const response = computeCombinedResponseDb(bands, FREQ_ARRAY);
    ctx.beginPath();
    for (let i = 0; i < CURVE_POINTS; i++) {
      const x = freqToX(FREQ_ARRAY[i], w);
      const db = Math.max(PARAMETRIC_DB_MIN - 2, Math.min(PARAMETRIC_DB_MAX + 2, response[i]));
      const y = dbToY(db, h);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.stroke();

    // Fill under the curve
    const y0 = dbToY(0, h);
    ctx.lineTo(freqToX(FREQ_ARRAY[CURVE_POINTS - 1], w), y0);
    ctx.lineTo(freqToX(FREQ_ARRAY[0], w), y0);
    ctx.closePath();
    const grad = ctx.createLinearGradient(0, MARGIN.top, 0, h - MARGIN.bottom);
    grad.addColorStop(0, accent);
    grad.addColorStop(1, accent);
    ctx.globalAlpha = 0.16;
    ctx.fillStyle = grad;
    ctx.fill();
    ctx.globalAlpha = 1;

    // Band nodes
    for (let i = 0; i < bands.length; i++) {
      const band = bands[i];
      const x = freqToX(band.frequencyHz, w);
      const hasGain = band.type !== 'highpass' && band.type !== 'lowpass';
      const y = hasGain ? dbToY(band.gainDb, h) : dbToY(0, h);
      const isSelected = i === selectedBandIndex;
      const color = isSelected ? accent : text;
      const alpha = band.enabled ? (isSelected ? 1 : 0.66) : 0.28;

      ctx.globalAlpha = alpha;
      ctx.beginPath();
      ctx.arc(x, y, isSelected ? NODE_RADIUS + 2 : NODE_RADIUS, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.fill();
      ctx.strokeStyle = text;
      ctx.lineWidth = isSelected ? 2 : 1;
      ctx.stroke();

      // Band label
      ctx.font = `bold 8px system-ui, sans-serif`;
      ctx.fillStyle = isSelected ? text : surface;
      ctx.textAlign = 'center';
      ctx.fillText(String(i + 1), x, y + 3);
      ctx.globalAlpha = 1;
    }

    ctx.restore();
  }, [bands, selectedBandIndex, accentColor]);

  // Resize canvas for devicePixelRatio
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    draw();
  }, [width, height, draw]);

  useEffect(() => { draw(); }, [draw]);

  const hitTestBand = useCallback((clientX: number, clientY: number): number => {
    const canvas = canvasRef.current;
    if (!canvas) return -1;
    const rect = canvas.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    const w = rect.width;
    const h = rect.height;
    let closest = -1;
    let closestDist = NODE_RADIUS + 6;
    for (let i = 0; i < bands.length; i++) {
      const band = bands[i];
      const nx = freqToX(band.frequencyHz, w);
      const hasGain = band.type !== 'highpass' && band.type !== 'lowpass';
      const ny = hasGain ? dbToY(band.gainDb, h) : dbToY(0, h);
      const dist = Math.sqrt((x - nx) ** 2 + (y - ny) ** 2);
      if (dist < closestDist) { closestDist = dist; closest = i; }
    }
    return closest;
  }, [bands]);

  const onPointerDown = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
    const hit = hitTestBand(e.clientX, e.clientY);
    if (hit >= 0) {
      onSelectBand(hit);
      const band = bands[hit];
      dragRef.current = {
        bandIndex: hit,
        startX: e.clientX,
        startY: e.clientY,
        startFreq: band.frequencyHz,
        startGain: band.gainDb,
      };
      (e.currentTarget as HTMLCanvasElement).setPointerCapture(e.pointerId);
      e.preventDefault();
    }
  }, [hitTestBand, bands, onSelectBand]);

  const onPointerMove = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const w = rect.width;
    const h = rect.height;

    const dx = e.clientX - drag.startX;
    const dy = e.clientY - drag.startY;

    // Horizontal → frequency (log scale)
    const logMin = Math.log10(PARAMETRIC_FREQ_MIN);
    const logMax = Math.log10(PARAMETRIC_FREQ_MAX);
    const logRange = logMax - logMin;
    const innerW = w - MARGIN.left - MARGIN.right;
    const logDelta = (dx / innerW) * logRange;
    const newFreq = clampFreq(Math.pow(10, Math.log10(drag.startFreq) + logDelta));

    // Vertical → gain
    const band = bands[drag.bandIndex];
    const hasGain = band.type !== 'highpass' && band.type !== 'lowpass';
    const updates: Partial<Pick<ParametricEqBand, 'frequencyHz' | 'gainDb'>> = { frequencyHz: newFreq };
    if (hasGain) {
      const dbRange = PARAMETRIC_DB_MAX - PARAMETRIC_DB_MIN;
      const innerH = h - MARGIN.top - MARGIN.bottom;
      const dbDelta = -(dy / innerH) * dbRange;
      updates.gainDb = clampDb(drag.startGain + dbDelta);
    }
    onChange(drag.bandIndex, updates);
    e.preventDefault();
  }, [bands, onChange]);

  const onPointerUp = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
    dragRef.current = null;
    (e.currentTarget as HTMLCanvasElement).releasePointerCapture(e.pointerId);
  }, []);

  return (
    <canvas
      ref={canvasRef}
      style={{ display: 'block', borderRadius: 4, cursor: 'crosshair', touchAction: 'none' }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    />
  );
}
