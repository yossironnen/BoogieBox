/**
 * Defines Use Adaptive Accent behavior for BoogieBox.
 */

import { useEffect } from 'react';
import { FastAverageColor } from 'fast-average-color';

const fac = new FastAverageColor();
const colorCache = new Map<string, { primary: string; secondary: string }>();

function toLinear(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function relativeLuminance(r: number, g: number, b: number): number {
  return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const rr = r / 255, gg = g / 255, bb = b / 255;
  const max = Math.max(rr, gg, bb), min = Math.min(rr, gg, bb);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === rr) h = ((gg - bb) / d + (gg < bb ? 6 : 0)) / 6;
  else if (max === gg) h = ((bb - rr) / d + 2) / 6;
  else h = ((rr - gg) / d + 4) / 6;
  return [h, s, l];
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  if (s === 0) { const v = Math.round(l * 255); return [v, v, v]; }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const hue2rgb = (t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [
    Math.round(hue2rgb(h + 1 / 3) * 255),
    Math.round(hue2rgb(h) * 255),
    Math.round(hue2rgb(h - 1 / 3) * 255),
  ];
}

function toHex(r: number, g: number, b: number): string {
  return '#' + [r, g, b].map(v => Math.max(0, Math.min(255, v)).toString(16).padStart(2, '0')).join('');
}

function adjustContrast(r: number, g: number, b: number): [number, number, number] {
  const lum = relativeLuminance(r, g, b);
  const [h, s, l] = rgbToHsl(r, g, b);
  if (lum < 0.08) return hslToRgb(h, Math.max(s, 0.5), Math.max(l, 0.52));
  if (lum > 0.65) return hslToRgb(h, s * 0.7, Math.min(l, 0.72));
  return [r, g, b];
}

function applyColors(primary: string, secondary: string): void {
  document.documentElement.style.setProperty('--accent', primary);
  document.documentElement.style.setProperty('--accent-primary', primary);
  document.documentElement.style.setProperty('--accent-secondary', secondary);
}

function resetColors(): void {
  const root = document.documentElement;
  const baseAccent = getComputedStyle(root).getPropertyValue('--accent-base').trim();
  if (baseAccent) {
    root.style.setProperty('--accent', baseAccent);
    root.style.setProperty('--accent-primary', baseAccent);
    root.style.setProperty('--accent-secondary', baseAccent);
    return;
  }
  root.style.removeProperty('--accent');
  root.style.removeProperty('--accent-primary');
  root.style.removeProperty('--accent-secondary');
}

function sameImageSource(imageUrl: string, img: HTMLImageElement): boolean {
  try {
    const target = new URL(imageUrl, window.location.href).toString();
    const current = img.currentSrc || img.src;
    if (!current) return false;
    return new URL(current, window.location.href).toString() === target;
  } catch {
    return false;
  }
}

async function sampleAndApply(img: HTMLImageElement, imageUrl: string, cancelledRef: { current: boolean }) {
  const result = await fac.getColorAsync(img, { algorithm: 'dominant', mode: 'speed' });
  if (cancelledRef.current) return;
  const [r, g, b] = adjustContrast(result.value[0], result.value[1], result.value[2]);
  const primary = toHex(r, g, b);
  const [h, s, l] = rgbToHsl(r, g, b);
  const [sr, sg, sb] = hslToRgb(h, s * 0.6, Math.min(l + 0.18, 0.80));
  const secondary = toHex(sr, sg, sb);
  colorCache.set(imageUrl, { primary, secondary });
  applyColors(primary, secondary);
}

/** Use Adaptive Accent Enabled is part of this module's public API. */
export function useAdaptiveAccentEnabled(imageUrl: string | null, enabled: boolean, imageElement: HTMLImageElement | null = null): void {
  useEffect(() => {
    if (!enabled) {
      resetColors();
      return;
    }
    if (!imageUrl) return;

    const cancelledRef = { current: false };

    if (colorCache.has(imageUrl)) {
      const c = colorCache.get(imageUrl)!;
      applyColors(c.primary, c.secondary);
      return () => { cancelledRef.current = true; resetColors(); };
    }

    const existingImage = imageElement && sameImageSource(imageUrl, imageElement) ? imageElement : null;
    const handleExistingLoad = () => {
      sampleAndApply(existingImage!, imageUrl, cancelledRef).catch(() => {});
    };

    if (existingImage) {
      if (existingImage.complete && existingImage.naturalWidth > 0) {
        handleExistingLoad();
      } else {
        existingImage.addEventListener('load', handleExistingLoad, { once: true });
      }
      return () => {
        cancelledRef.current = true;
        existingImage.removeEventListener('load', handleExistingLoad);
        resetColors();
      };
    }

    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.src = imageUrl;

    img.onload = async () => {
      if (cancelledRef.current) return;
      try {
        await sampleAndApply(img, imageUrl, cancelledRef);
      } catch {
        // CORS or extraction failure - keep default accent
      }
    };

    return () => {
      cancelledRef.current = true;
      resetColors();
    };
  }, [enabled, imageElement, imageUrl]);
}
