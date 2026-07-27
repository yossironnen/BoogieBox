/**
 * Covers the guarded Hybrid preview configuration and theme mapping.
 */

import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS } from './types';
import {
  getClassicPreviewHref,
  getHybridSemanticTokens,
  DESKTOP_PLAYER_DOCK_HEIGHT,
  DESKTOP_PLAYER_POPUP_GAP,
  DESKTOP_VINYL_PLAYER_DOCK_HEIGHT,
  hybridAudioPanelStyles,
  hybridControlStyles,
  hybridMediaStyles,
  hybridPlayerStyles,
  hybridPlaylistStyles,
  hybridSettingsStyles,
  HYBRID_FONT_FAMILY,
  HYBRID_FONT_STYLESHEET_HREF,
  HYBRID_FONT_STYLESHEET_ID,
  HYBRID_SEMANTIC_TOKEN_KEYS,
  mountHybridFont,
  parseHybridPreview,
  parseHybridThemeMode,
  resolveHybridThemeSettings,
} from './hybridPreview';

describe('Hybrid preview', () => {
  it('stays disabled without the explicit development query flag', () => {
    expect(parseHybridPreview('', true)).toEqual({ enabled: false, mode: 'dark' });
    expect(parseHybridPreview('?ui-preview=hybrid', false)).toEqual({ enabled: false, mode: 'dark' });
  });

  it('accepts only the supported temporary preview modes', () => {
    expect(parseHybridPreview('?ui-preview=hybrid&ui-preview-theme=light', true))
      .toEqual({ enabled: true, mode: 'light' });
    expect(parseHybridPreview('?ui-preview=hybrid&ui-preview-theme=custom', true))
      .toEqual({ enabled: true, mode: 'custom' });
    expect(parseHybridPreview('?ui-preview=hybrid&ui-preview-theme=neon', true))
      .toEqual({ enabled: true, mode: 'dark' });
  });

  it('validates persisted production theme modes', () => {
    expect(parseHybridThemeMode('light')).toBe('light');
    expect(parseHybridThemeMode('dark')).toBe('dark');
    expect(parseHybridThemeMode('custom')).toBe('custom');
    expect(parseHybridThemeMode('neon')).toBeNull();
    expect(parseHybridThemeMode(null)).toBeNull();
  });

  it('maps every Hybrid theme to Satoshi without changing the saved settings', () => {
    const saved = { ...DEFAULT_SETTINGS };
    const light = resolveHybridThemeSettings(DEFAULT_SETTINGS, 'light');
    const dark = resolveHybridThemeSettings(DEFAULT_SETTINGS, 'dark');
    const custom = resolveHybridThemeSettings(DEFAULT_SETTINGS, 'custom');

    expect(light.colorBg).toBe('#f7f5f2');
    expect(dark.colorBg).toBe('#141211');
    expect(light.fontFamily).toBe(HYBRID_FONT_FAMILY);
    expect(dark.fontFamily).toBe(HYBRID_FONT_FAMILY);
    expect(custom.fontFamily).toBe(HYBRID_FONT_FAMILY);
    expect(DEFAULT_SETTINGS).toEqual(saved);
  });

  it('uses the saved scheme unchanged for Custom while deriving semantic roles', () => {
    const custom = {
      ...DEFAULT_SETTINGS,
      colorBg: '#102030',
      colorSurface: '#203040',
      colorAccent: '#abcdef',
    };

    expect(resolveHybridThemeSettings(custom, 'custom')).toEqual({
      ...custom,
      fontFamily: HYBRID_FONT_FAMILY,
    });
    const tokens = getHybridSemanticTokens(custom, 'custom');
    expect(Object.keys(tokens)).toEqual([...HYBRID_SEMANTIC_TOKEN_KEYS]);
    expect(tokens['--focus']).toBe('#abcdef');
    expect(tokens['--focus-ring']).toContain('#abcdef');
    expect(tokens['--surface-raised']).toContain('#203040');
    expect(tokens['--danger']).toBe('#d65c5c');
  });

  it('provides complete accessible semantic roles for Light and Dark', () => {
    const light = getHybridSemanticTokens(DEFAULT_SETTINGS, 'light');
    const dark = getHybridSemanticTokens(DEFAULT_SETTINGS, 'dark');

    expect(Object.keys(light)).toEqual([...HYBRID_SEMANTIC_TOKEN_KEYS]);
    expect(Object.keys(dark)).toEqual([...HYBRID_SEMANTIC_TOKEN_KEYS]);
    expect(light['--surface-raised']).toBe('#ffffff');
    expect(light['--overlay']).toContain('0.48');
    expect(dark['--text-faint']).toBe('#887a72');
    expect(dark['--focus-ring']).toContain('0.28');
  });

  it('defines flat reusable control and media roles for rollout screens', () => {
    expect(hybridControlStyles.primaryButton).toMatchObject({
      background: 'var(--accent)',
      color: 'var(--on-accent)',
      boxShadow: 'none',
    });
    expect(hybridControlStyles.dangerButton.color).toBe('var(--danger)');
    expect(hybridControlStyles.field.borderRadius).toBe(10);
    expect(hybridControlStyles.switchTrack).toMatchObject({
      width: 44,
      borderRadius: 999,
    });
    expect(hybridMediaStyles.listRow).toMatchObject({
      border: 'none',
      boxShadow: 'none',
    });
    expect(hybridPlaylistStyles.sidebarItemActive).toMatchObject({
      background: 'var(--accent-soft)',
      transform: 'none',
    });
    expect(hybridSettingsStyles.tabActive).toMatchObject({
      background: 'var(--surface-raised)',
      color: 'var(--accent)',
    });
    expect(hybridSettingsStyles.panel).toMatchObject({
      borderRadius: 14,
      boxShadow: 'none',
    });
    expect(hybridAudioPanelStyles.popup).toMatchObject({
      borderRadius: 16,
      background: 'var(--surface-raised)',
      boxShadow: 'var(--shadow-raised)',
    });
    expect(hybridAudioPanelStyles.listRowActive).toMatchObject({
      background: 'var(--accent-soft)',
    });
  });

  it('keeps the Hybrid player tall enough for its 90px meters and docked popups', () => {
    expect(DESKTOP_PLAYER_DOCK_HEIGHT).toBe(100);
    expect(DESKTOP_VINYL_PLAYER_DOCK_HEIGHT).toBe(170);
    expect(DESKTOP_PLAYER_POPUP_GAP).toBe(8);
    expect(hybridPlayerStyles.bar).toMatchObject({
      height: DESKTOP_PLAYER_DOCK_HEIGHT,
      minHeight: DESKTOP_PLAYER_DOCK_HEIGHT,
    });
  });

  it('mounts one removable official Satoshi stylesheet', () => {
    document.getElementById(HYBRID_FONT_STYLESHEET_ID)?.remove();

    const unmount = mountHybridFont();
    const link = document.getElementById(HYBRID_FONT_STYLESHEET_ID);

    expect(link).toHaveAttribute('rel', 'stylesheet');
    expect(link).toHaveAttribute('href', HYBRID_FONT_STYLESHEET_HREF);
    mountHybridFont();
    expect(document.querySelectorAll(`#${HYBRID_FONT_STYLESHEET_ID}`)).toHaveLength(1);

    unmount();
    expect(document.getElementById(HYBRID_FONT_STYLESHEET_ID)).toBeNull();
  });

  it('builds an exit URL without disturbing unrelated location state', () => {
    expect(getClassicPreviewHref('http://localhost:3000/browse?ui-preview=hybrid&foo=1&ui-preview-theme=light#top'))
      .toBe('/browse?foo=1#top');
  });
});
