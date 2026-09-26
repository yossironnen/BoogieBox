/**
 * Tests the Vintage theme registry: parsing, token completeness, WCAG contrast of
 * each style's key text pairs, and root token apply/cleanup.
 */

import { afterEach, describe, expect, it } from 'vitest';
import { HYBRID_SEMANTIC_TOKEN_KEYS } from './hybridPreview';
import {
  applyVintageTokens,
  contrastRatio,
  DEFAULT_VINTAGE_STYLE,
  getVintagePageVars,
  getVintageStyle,
  mountVintageFonts,
  parseVintageStyle,
  VINTAGE_STYLE_IDS,
  VINTAGE_STYLES,
  VINTAGE_TOKEN_KEYS,
  type VintageStyleDefinition,
} from './vintageThemes';

const ALL_STYLES = VINTAGE_STYLE_IDS.map((id) => VINTAGE_STYLES[id]);

describe('parseVintageStyle / getVintageStyle', () => {
  it('accepts registered style ids only', () => {
    expect(parseVintageStyle('recordshop')).toBe('recordshop');
    expect(parseVintageStyle('walnut')).toBeNull();
    expect(parseVintageStyle('')).toBeNull();
    expect(parseVintageStyle(null)).toBeNull();
    expect(parseVintageStyle(42)).toBeNull();
  });

  it('falls back to the default style', () => {
    expect(DEFAULT_VINTAGE_STYLE).toBe('recordshop');
    expect(getVintageStyle(null).id).toBe(DEFAULT_VINTAGE_STYLE);
    expect(getVintageStyle(undefined).id).toBe(DEFAULT_VINTAGE_STYLE);
    expect(getVintageStyle('recordshop')).toBe(VINTAGE_STYLES.recordshop);
  });
});

describe.each(ALL_STYLES.map((def) => [def.id, def] as const))('vintage style %s', (_id, def: VintageStyleDefinition) => {
  it('defines every semantic and vintage token', () => {
    expect(Object.keys(def.semanticTokens).sort()).toEqual([...HYBRID_SEMANTIC_TOKEN_KEYS].sort());
    expect(Object.keys(def.tokens).sort()).toEqual([...VINTAGE_TOKEN_KEYS].sort());
    for (const value of [...Object.values(def.semanticTokens), ...Object.values(def.tokens)]) {
      expect(value.trim()).not.toBe('');
    }
    expect(def.stripes.length).toBeGreaterThan(0);
    expect(def.stickerShadows.length).toBeGreaterThan(0);
  });

  it('keeps page text readable (WCAG AA)', () => {
    const { colorBg, colorSurface, colorText, colorTextMuted, colorAccent } = def.palette;
    for (const bg of [colorBg, colorSurface]) {
      expect(contrastRatio(colorText, bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(colorTextMuted, bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(colorAccent, bg)).toBeGreaterThanOrEqual(4.5);
    }
    // White/on-accent text on accent-filled buttons.
    expect(contrastRatio('#ffffff', colorAccent)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(def.semanticTokens['--on-accent'], colorAccent)).toBeGreaterThanOrEqual(4.5);
    // Active crate tab: on-accent text on teal.
    expect(contrastRatio(def.semanticTokens['--on-accent'], def.tokens['--vintage-teal'])).toBeGreaterThanOrEqual(4.5);
  });

  it('keeps player dock text readable (WCAG AA)', () => {
    const dock = def.dockVars;
    expect(contrastRatio(dock['--text'], dock['--bg'])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(dock['--text-muted'], dock['--bg'])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(dock['--accent'], dock['--bg'])).toBeGreaterThanOrEqual(4.5);
    // Play button: dock background colour as the icon on stripe 2.
    expect(contrastRatio(dock['--bg'], def.tokens['--vintage-stripe-2'])).toBeGreaterThanOrEqual(3);
  });

  it('exposes the page palette for popups inside the dark dock', () => {
    const vars = getVintagePageVars(def);
    expect(vars['--bg']).toBe(def.palette.colorBg);
    expect(vars['--text']).toBe(def.palette.colorText);
    expect(vars['--surface-raised']).toBe(def.semanticTokens['--surface-raised']);
  });
});

describe('contrastRatio', () => {
  it('matches known WCAG values', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrastRatio('#ffffff', '#ffffff')).toBeCloseTo(1, 5);
    expect(() => contrastRatio('red', '#fff')).toThrow();
  });
});

describe('applyVintageTokens', () => {
  afterEach(() => applyVintageTokens(null));

  it('writes tokens and data-vintage-style, then removes them all', () => {
    const root = document.documentElement;
    applyVintageTokens(VINTAGE_STYLES.recordshop);
    expect(root.dataset.vintageStyle).toBe('recordshop');
    expect(root.style.getPropertyValue('--vu-face')).toBe('#f2c57a');
    expect(root.style.getPropertyValue('--vintage-teal')).toBe('#2e6e6a');

    applyVintageTokens(null);
    expect(root.dataset.vintageStyle).toBeUndefined();
    for (const key of VINTAGE_TOKEN_KEYS) {
      expect(root.style.getPropertyValue(key)).toBe('');
    }
  });
});

describe('mountVintageFonts', () => {
  it('never rejects, even when a font chunk fails to load', async () => {
    const failing: VintageStyleDefinition = {
      ...VINTAGE_STYLES.recordshop,
      loadFonts: () => Promise.reject(new Error('offline')),
    };
    await expect(mountVintageFonts(failing)).resolves.toBeUndefined();
    await expect(mountVintageFonts(VINTAGE_STYLES.recordshop)).resolves.toBeUndefined();
  });
});
