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

/** Resolves a plain `var(--x)` against the given custom-property scope. */
function resolveColor(value: unknown, scope: Record<string, string>): string {
  const match = /^var\((--[\w-]+)\)$/.exec(String(value));
  return match ? scope[match[1]] : String(value);
}

describe('parseVintageStyle / getVintageStyle', () => {
  it('accepts registered style ids only', () => {
    expect(parseVintageStyle('recordshop')).toBe('recordshop');
    expect(parseVintageStyle('hificonsole')).toBe('hificonsole');
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
    // The stripe band keys its rows by colour.
    expect(new Set(def.stripes).size).toBe(def.stripes.length);
  });

  it('keeps page text readable (WCAG AA)', () => {
    const { colorBg, colorSurface, colorText, colorTextMuted, colorAccent } = def.palette;
    for (const bg of [colorBg, colorSurface]) {
      expect(contrastRatio(colorText, bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(colorTextMuted, bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(colorAccent, bg)).toBeGreaterThanOrEqual(4.5);
    }
    // On-accent text on accent-filled buttons (light styles also take plain white).
    expect(contrastRatio(def.semanticTokens['--on-accent'], colorAccent)).toBeGreaterThanOrEqual(4.5);
    if (contrastRatio(colorBg, '#000000') > contrastRatio(colorBg, '#ffffff')) {
      expect(contrastRatio('#ffffff', colorAccent)).toBeGreaterThanOrEqual(4.5);
    }
    // Active sidebar tab, expanded and collapsed.
    const scope = { ...getVintagePageVars(def), ...def.tokens };
    for (const active of [def.styles.navItemActive, def.styles.navItemActiveCollapsed]) {
      const text = resolveColor(active.color, scope);
      const bg = resolveColor(active.backgroundColor, scope);
      expect(contrastRatio(text, bg)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('keeps player dock text readable (WCAG AA)', () => {
    const dock = def.dockVars;
    expect(contrastRatio(dock['--text'], dock['--bg'])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(dock['--text-muted'], dock['--bg'])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(dock['--accent'], dock['--bg'])).toBeGreaterThanOrEqual(4.5);
    // Play button glyph on its face.
    const scope = { ...getVintagePageVars(def), ...def.tokens, ...dock };
    const play = def.styles.dockPlayBtn;
    expect(contrastRatio(resolveColor(play.color, scope), resolveColor(play.backgroundColor, scope))).toBeGreaterThanOrEqual(3);
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

    applyVintageTokens(VINTAGE_STYLES.hificonsole);
    expect(root.dataset.vintageStyle).toBe('hificonsole');
    expect(root.style.getPropertyValue('--vu-face')).toBe('#7cc4f0');

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
    await expect(mountVintageFonts(VINTAGE_STYLES.hificonsole)).resolves.toBeUndefined();
  });
});
