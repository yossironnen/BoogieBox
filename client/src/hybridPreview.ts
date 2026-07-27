/**
 * Defines the approved Hybrid design foundation and its guarded preview controls.
 */

import type React from 'react';
import type { AppSettings } from './types';

export type HybridThemeMode = 'light' | 'dark' | 'custom';

export interface HybridPreviewConfig {
  enabled: boolean;
  mode: HybridThemeMode;
}

export const HYBRID_THEME_MODES: HybridThemeMode[] = ['light', 'dark', 'custom'];
const HYBRID_THEME_MODE_SET = new Set<HybridThemeMode>(HYBRID_THEME_MODES);

export const HYBRID_FONT_FAMILY =
  "'Satoshi', Aptos, \"Segoe UI Variable\", \"Segoe UI\", Inter, system-ui, sans-serif";
export const HYBRID_FONT_STYLESHEET_ID = 'boogiebox-hybrid-preview-font';
export const HYBRID_FONT_STYLESHEET_HREF =
  'https://api.fontshare.com/v2/css?f[]=satoshi@300,400,500,600,700,900&display=swap';

export const DESKTOP_PLAYER_DOCK_HEIGHT = 100;
export const DESKTOP_VINYL_PLAYER_DOCK_HEIGHT = 170;
export const DESKTOP_PLAYER_POPUP_GAP = 8;

const HYBRID_LIGHT = {
  colorBg: '#f7f5f2',
  colorSurface: '#ffffff',
  colorBorder: '#e4ddd5',
  colorAccent: '#a94f2a',
  colorText: '#221d19',
  colorTextMuted: '#71665e',
} as const;

const HYBRID_DARK = {
  colorBg: '#141211',
  colorSurface: '#1d1a18',
  colorBorder: '#332d29',
  colorAccent: '#e39162',
  colorText: '#f4eee9',
  colorTextMuted: '#b9aaa0',
} as const;

export const HYBRID_SEMANTIC_TOKEN_KEYS = [
  '--surface-raised',
  '--surface-subtle',
  '--surface-hover',
  '--divider-subtle',
  '--border-strong',
  '--text-faint',
  '--accent-soft',
  '--accent-secondary',
  '--on-accent',
  '--focus',
  '--focus-ring',
  '--success',
  '--warning',
  '--danger',
  '--overlay',
  '--shadow-subtle',
  '--shadow-raised',
] as const;

export function parseHybridPreview(
  search: string,
  development = import.meta.env.DEV,
): HybridPreviewConfig {
  const params = new URLSearchParams(search.startsWith('?') ? search.slice(1) : search);
  const requestedMode = parseHybridThemeMode(params.get('ui-preview-theme'));
  return {
    enabled: development && params.get('ui-preview') === 'hybrid',
    mode: requestedMode ?? 'dark',
  };
}

export function parseHybridThemeMode(value: unknown): HybridThemeMode | null {
  return typeof value === 'string' && HYBRID_THEME_MODE_SET.has(value as HybridThemeMode)
    ? value as HybridThemeMode
    : null;
}

export function resolveHybridThemeSettings(
  settings: AppSettings,
  mode: HybridThemeMode,
): AppSettings {
  if (mode === 'custom') {
    return {
      ...settings,
      fontFamily: HYBRID_FONT_FAMILY,
    };
  }
  return {
    ...settings,
    ...(mode === 'light' ? HYBRID_LIGHT : HYBRID_DARK),
    bgTexture: 'none',
    fontFamily: HYBRID_FONT_FAMILY,
  };
}

export function mountHybridFont(doc: Document = document): () => void {
  const existing = doc.getElementById(HYBRID_FONT_STYLESHEET_ID);
  if (existing) return () => {};

  const link = doc.createElement('link');
  link.id = HYBRID_FONT_STYLESHEET_ID;
  link.rel = 'stylesheet';
  link.href = HYBRID_FONT_STYLESHEET_HREF;
  doc.head.appendChild(link);
  return () => link.remove();
}

export function getHybridSemanticTokens(
  settings: AppSettings,
  mode: HybridThemeMode,
): Record<(typeof HYBRID_SEMANTIC_TOKEN_KEYS)[number], string> {
  if (mode === 'light') {
    return {
      '--surface-raised': '#ffffff',
      '--surface-subtle': '#efeae4',
      '--surface-hover': '#e9e3dc',
      '--divider-subtle': '#ebe5df',
      '--border-strong': '#d3c8bd',
      '--text-faint': '#9a8d83',
      '--accent-soft': '#f6e3d8',
      '--accent-secondary': '#d39a50',
      '--on-accent': '#ffffff',
      '--focus': '#a94f2a',
      '--focus-ring': '0 0 0 3px rgba(169,79,42,0.24)',
      '--success': '#2f7d57',
      '--warning': '#9a641d',
      '--danger': '#b43d3d',
      '--overlay': 'rgba(34,29,25,0.48)',
      '--shadow-subtle': '0 2px 8px rgba(48,35,26,0.07)',
      '--shadow-raised': '0 18px 42px rgba(48,35,26,0.14)',
    };
  }
  if (mode === 'dark') {
    return {
      '--surface-raised': '#231f1c',
      '--surface-subtle': '#27221f',
      '--surface-hover': '#302925',
      '--divider-subtle': '#2c2724',
      '--border-strong': '#463c36',
      '--text-faint': '#887a72',
      '--accent-soft': '#3a251b',
      '--accent-secondary': '#e2b266',
      '--on-accent': '#241108',
      '--focus': '#e39162',
      '--focus-ring': '0 0 0 3px rgba(227,145,98,0.28)',
      '--success': '#65c590',
      '--warning': '#e3b05f',
      '--danger': '#f07b76',
      '--overlay': 'rgba(0,0,0,0.66)',
      '--shadow-subtle': '0 2px 8px rgba(0,0,0,0.34)',
      '--shadow-raised': '0 20px 46px rgba(0,0,0,0.48)',
    };
  }
  return {
    '--surface-raised': `color-mix(in srgb, ${settings.colorSurface} 94%, ${settings.colorText})`,
    '--surface-subtle': `color-mix(in srgb, ${settings.colorSurface} 76%, ${settings.colorBg})`,
    '--surface-hover': `color-mix(in srgb, ${settings.colorSurface} 82%, ${settings.colorText})`,
    '--divider-subtle': `color-mix(in srgb, ${settings.colorBorder} 64%, transparent)`,
    '--border-strong': `color-mix(in srgb, ${settings.colorBorder} 72%, ${settings.colorText})`,
    '--text-faint': `color-mix(in srgb, ${settings.colorTextMuted} 72%, ${settings.colorBg})`,
    '--accent-soft': `color-mix(in srgb, ${settings.colorAccent} 16%, ${settings.colorBg})`,
    '--accent-secondary': `color-mix(in srgb, ${settings.colorAccent} 72%, ${settings.colorTextMuted})`,
    '--on-accent': '#ffffff',
    '--focus': settings.colorAccent,
    '--focus-ring': `0 0 0 3px color-mix(in srgb, ${settings.colorAccent} 28%, transparent)`,
    '--success': '#4f9d73',
    '--warning': '#d49a45',
    '--danger': '#d65c5c',
    '--overlay': 'rgba(0,0,0,0.56)',
    '--shadow-subtle': '0 2px 8px rgba(0,0,0,0.12)',
    '--shadow-raised': '0 18px 42px rgba(0,0,0,0.24)',
  };
}

export function getClassicPreviewHref(href: string): string {
  const url = new URL(href);
  url.searchParams.delete('ui-preview');
  url.searchParams.delete('ui-preview-theme');
  return `${url.pathname}${url.search}${url.hash}`;
}

export const hybridShellStyles: Record<string, React.CSSProperties> = {
  root: {
    backgroundImage: 'none',
  },
  sidebar: {
    backgroundColor: 'var(--bg)',
    borderRight: '1px solid var(--border)',
    boxShadow: 'none',
  },
  logo: {
    borderBottom: 'none',
    paddingTop: 20,
    paddingBottom: 18,
  },
  navItem: {
    borderRadius: 10,
  },
  navItemActive: {
    backgroundColor: 'transparent',
    color: 'var(--accent)',
    boxShadow: 'inset 3px 0 0 var(--accent)',
  },
  main: {
    backgroundColor: 'var(--bg)',
  },
  previewBar: {
    position: 'fixed',
    top: 14,
    right: 18,
    zIndex: 180,
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    padding: '7px 8px 7px 12px',
    border: '1px solid var(--border-strong)',
    borderRadius: 999,
    background: 'color-mix(in srgb, var(--surface) 90%, transparent)',
    backdropFilter: 'blur(16px) saturate(1.2)',
    boxShadow: 'var(--shadow-subtle)',
  },
  previewLabel: {
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 700,
    letterSpacing: 0.5,
    textTransform: 'uppercase',
  },
  previewModes: {
    display: 'flex',
    alignItems: 'center',
    gap: 2,
    padding: 2,
    borderRadius: 999,
    background: 'var(--surface-subtle)',
  },
  previewMode: {
    padding: '5px 9px',
    border: 'none',
    borderRadius: 999,
    background: 'transparent',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 11,
    cursor: 'pointer',
  },
  previewModeActive: {
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    boxShadow: 'var(--shadow-subtle)',
  },
  classicLink: {
    padding: '4px 7px',
    color: 'var(--accent)',
    fontSize: 11,
    fontWeight: 650,
    textDecoration: 'none',
  },
};

export const hybridBrowseStyles: Record<string, React.CSSProperties> = {
  root: {
    background: 'var(--bg)',
  },
  hero: {
    background: 'transparent',
    borderBottom: 'none',
    boxShadow: 'none',
  },
  heroInner: {
    padding: '24px 28px 16px',
  },
  heroTitle: {
    fontSize: 26,
    fontWeight: 750,
    letterSpacing: -0.55,
  },
  heroBody: {
    maxWidth: 620,
    marginTop: 7,
    color: 'var(--text-muted)',
    fontSize: 13,
  },
  heroStats: {
    minWidth: 164,
    padding: '11px 14px',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
    border: 'none',
    boxShadow: 'none',
  },
  toolbar: {
    padding: '0 28px 14px',
    borderBottom: '1px solid var(--border)',
  },
  gridTile: {
    padding: 0,
    border: 'none',
    borderRadius: 14,
    background: 'transparent',
  },
  gridArt: {
    border: 'none',
    borderRadius: 14,
    boxShadow: 'var(--shadow-subtle)',
  },
  row: {
    margin: '2px 12px',
    borderBottom: 'none',
    borderRadius: 10,
  },
};

export const hybridHomeStyles: Record<string, React.CSSProperties> = {
  root: {
    padding: '24px 28px 32px',
    background: 'var(--bg)',
  },
  grid: {
    gap: 16,
  },
  card: {
    padding: '18px 20px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'var(--surface)',
    boxShadow: 'none',
  },
  cardTitle: {
    fontSize: 17,
    fontWeight: 750,
    letterSpacing: -0.3,
  },
};

export const hybridSearchStyles: Record<string, React.CSSProperties> = {
  root: {
    background: 'var(--bg)',
  },
  filterBar: {
    gap: 10,
    padding: '24px 28px 18px',
    borderBottom: '1px solid var(--divider-subtle)',
    background: 'transparent',
  },
  heroCopy: {
    width: '100%',
    marginBottom: 4,
  },
  heroTitle: {
    color: 'var(--text)',
    fontSize: 26,
    fontWeight: 750,
    letterSpacing: -0.55,
  },
  heroBody: {
    maxWidth: 620,
    marginTop: 6,
    color: 'var(--text-muted)',
    fontSize: 13,
    lineHeight: 1.5,
  },
  searchWrap: {
    minHeight: 46,
    background: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 12,
    boxShadow: 'none',
  },
  searchWrapFocused: {
    borderColor: 'var(--focus)',
    boxShadow: 'var(--focus-ring)',
  },
  select: {
    minHeight: 46,
    background: 'var(--surface-subtle)',
    border: '1px solid transparent',
    borderRadius: 12,
  },
  quickPanel: {
    padding: '14px 28px 16px',
    borderBottom: '1px solid var(--divider-subtle)',
    background: 'transparent',
  },
  quickSection: {
    padding: '5px 0',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
    boxShadow: 'none',
  },
  quickRow: {
    padding: '9px 14px',
  },
  resultsMeta: {
    padding: '12px 28px 8px',
  },
  tableHeader: {
    padding: '10px 28px',
    borderBottom: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
  },
  tableRow: {
    margin: '2px 16px',
    padding: '10px 12px',
    borderBottom: 'none',
    borderRadius: 10,
  },
  sectionDivider: {
    padding: '8px 28px 2px',
  },
};

export const hybridControlStyles: Record<string, React.CSSProperties> = {
  primaryButton: {
    minHeight: 38,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
    padding: '9px 16px',
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    boxShadow: 'none',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 700,
  },
  secondaryButton: {
    minHeight: 38,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
    padding: '9px 13px',
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    boxShadow: 'none',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 650,
  },
  tonalButton: {
    minHeight: 38,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
    padding: '9px 13px',
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    boxShadow: 'none',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 700,
  },
  dangerButton: {
    minHeight: 38,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
    padding: '9px 13px',
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'color-mix(in srgb, var(--danger) 14%, transparent)',
    color: 'var(--danger)',
    boxShadow: 'none',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 700,
  },
  iconButton: {
    width: 38,
    minWidth: 38,
    height: 38,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 0,
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    boxShadow: 'none',
    cursor: 'pointer',
  },
  field: {
    minHeight: 42,
    padding: '9px 12px',
    border: '1px solid var(--border)',
    borderRadius: 10,
    background: 'var(--surface)',
    color: 'var(--text)',
    outline: 'none',
    fontFamily: 'inherit',
    fontSize: 13,
  },
  select: {
    minHeight: 38,
    padding: '8px 32px 8px 11px',
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    outline: 'none',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 650,
  },
  segmentedGroup: {
    display: 'inline-flex',
    gap: 2,
    padding: 3,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
  },
  segment: {
    minHeight: 32,
    padding: '6px 12px',
    border: 'none',
    borderRadius: 8,
    background: 'transparent',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 650,
  },
  segmentActive: {
    background: 'var(--surface-raised)',
    color: 'var(--accent)',
  },
  disabled: {
    cursor: 'not-allowed',
    opacity: 0.45,
  },
  switchTrack: {
    width: 44,
    minWidth: 44,
    height: 24,
    position: 'relative',
    padding: 0,
    border: '1px solid transparent',
    borderRadius: 999,
    background: 'var(--border-strong)',
    cursor: 'pointer',
    transition: 'background 0.16s ease',
  },
  switchTrackActive: {
    background: 'var(--accent)',
  },
  switchThumb: {
    width: 18,
    height: 18,
    position: 'absolute',
    top: 2,
    left: 2,
    borderRadius: '50%',
    background: 'var(--surface-raised)',
    boxShadow: 'var(--shadow-subtle)',
    transition: 'transform 0.16s ease',
  },
  switchThumbActive: {
    transform: 'translateX(20px)',
  },
};

export const hybridMediaStyles: Record<string, React.CSSProperties> = {
  artworkFrame: {
    overflow: 'hidden',
    border: 'none',
    borderRadius: 16,
    background: 'var(--surface-subtle)',
    boxShadow: 'var(--shadow-subtle)',
  },
  listRow: {
    margin: '2px 12px',
    padding: '9px 12px',
    border: 'none',
    borderRadius: 10,
    background: 'transparent',
    boxShadow: 'none',
  },
  sidePanel: {
    borderLeft: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
    boxShadow: 'none',
  },
  emptyState: {
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'var(--surface)',
    boxShadow: 'none',
  },
  overlay: {
    background: 'var(--overlay)',
  },
  dialog: {
    border: '1px solid var(--border-strong)',
    borderRadius: 16,
    background: 'var(--surface-raised)',
    boxShadow: 'var(--shadow-raised)',
  },
};

export const hybridPlaylistStyles: Record<string, React.CSSProperties> = {
  root: {
    background: 'var(--bg)',
  },
  sidebar: {
    width: 232,
    borderRight: '1px solid var(--divider-subtle)',
    background: 'var(--bg)',
  },
  sidebarHeader: {
    padding: '20px 16px 14px',
    borderBottom: 'none',
  },
  sidebarItem: {
    width: 'calc(100% - 16px)',
    margin: '4px 8px 0',
    padding: '11px 12px',
    border: '1px solid transparent',
    borderRadius: 10,
    background: 'transparent',
    transform: 'none',
  },
  sidebarItemActive: {
    borderColor: 'transparent',
    background: 'var(--accent-soft)',
    boxShadow: 'inset 3px 0 0 var(--accent)',
    transform: 'none',
  },
  detailHeader: {
    gap: 20,
    padding: '24px 28px 20px',
    borderBottom: '1px solid var(--divider-subtle)',
    background: 'transparent',
    boxShadow: 'none',
  },
  detailName: {
    fontSize: 30,
    fontWeight: 800,
    letterSpacing: -0.8,
  },
  actionGroup: {
    display: 'flex',
    gap: 8,
    alignItems: 'center',
    flexShrink: 0,
    flexWrap: 'wrap',
  },
  statusPanel: {
    margin: '10px 16px',
    padding: '12px 14px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 12,
    background: 'var(--surface-subtle)',
  },
  crossfadePanel: {
    padding: '12px 28px',
    borderBottom: '1px solid var(--divider-subtle)',
    background: 'var(--surface)',
  },
};

export const hybridSettingsStyles: Record<string, React.CSSProperties> = {
  page: {
    width: '100%',
    maxWidth: 1040,
    padding: '28px 36px 40px',
    background: 'var(--bg)',
  },
  pageHeader: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 20,
    marginBottom: 20,
  },
  title: {
    margin: 0,
    color: 'var(--text)',
    fontSize: 28,
    fontWeight: 800,
    letterSpacing: -0.7,
    lineHeight: 1.1,
  },
  subtitle: {
    marginTop: 6,
    color: 'var(--text-muted)',
    fontSize: 12,
    lineHeight: 1.5,
  },
  account: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: 4,
    borderRadius: 12,
    background: 'var(--surface-subtle)',
  },
  roleBadge: {
    padding: '4px 8px',
    borderRadius: 999,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 10,
    fontWeight: 750,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
  },
  tabBar: {
    display: 'flex',
    gap: 3,
    marginBottom: 24,
    padding: 4,
    overflowX: 'auto',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 13,
    background: 'var(--surface-subtle)',
  },
  tab: {
    minHeight: 36,
    flex: '0 0 auto',
    padding: '8px 13px',
    border: 'none',
    borderRadius: 9,
    background: 'transparent',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 650,
    transition: 'background 0.12s ease, color 0.12s ease',
  },
  tabActive: {
    background: 'var(--surface-raised)',
    color: 'var(--accent)',
    boxShadow: 'var(--shadow-subtle)',
  },
  section: {
    paddingTop: 0,
  },
  sectionTitle: {
    marginBottom: 12,
    color: 'var(--text)',
    fontSize: 15,
    fontWeight: 750,
    letterSpacing: -0.1,
    textTransform: 'none',
  },
  panel: {
    marginBottom: 14,
    padding: '18px 20px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
    boxShadow: 'none',
  },
  panelHeader: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 16,
    marginBottom: 16,
  },
  panelDescription: {
    maxWidth: 680,
    marginTop: 4,
    color: 'var(--text-muted)',
    fontSize: 11,
    lineHeight: 1.55,
  },
  advancedIntro: {
    marginBottom: 16,
    padding: '16px 18px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  advancedNav: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 6,
    marginTop: 12,
  },
  advancedNavItem: {
    padding: '7px 10px',
    border: '1px solid transparent',
    borderRadius: 9,
    background: 'var(--surface-raised)',
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 650,
    textDecoration: 'none',
  },
};

export const hybridPlayerStyles: Record<string, React.CSSProperties> = {
  bar: {
    height: DESKTOP_PLAYER_DOCK_HEIGHT,
    minHeight: DESKTOP_PLAYER_DOCK_HEIGHT,
    padding: '0 18px',
    background: 'color-mix(in srgb, var(--surface) 88%, transparent)',
    backdropFilter: 'blur(16px) saturate(1.2)',
    borderTop: '1px solid var(--border)',
    boxShadow: '0 -10px 28px rgba(0,0,0,0.12)',
  },
  albumArtWrap: {
    width: 54,
    height: 54,
    borderRadius: 14,
    border: 'none',
    boxShadow: 'var(--shadow-subtle)',
  },
};
