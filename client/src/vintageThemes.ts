/**
 * Vintage theme registry: palettes, tokens, fonts and surface styles for the
 * desktop "Vintage" theme mode. Record Shop '74 is the first style; further
 * styles are added as one more entry in VINTAGE_STYLES (plus the matching id in
 * the server's UI_VINTAGE_STYLES allowlist).
 */

import { createContext, useContext } from 'react';
import type React from 'react';
import type { HYBRID_SEMANTIC_TOKEN_KEYS } from './hybridPreview';

/** Vintage Style is part of this module's public API. */
export type VintageStyle = 'recordshop';

/** Vintage style ids in picker order. */
export const VINTAGE_STYLE_IDS: readonly VintageStyle[] = ['recordshop'];

/** Style used when none (or an unknown one) is stored. */
export const DEFAULT_VINTAGE_STYLE: VintageStyle = 'recordshop';

type SemanticTokens = Record<(typeof HYBRID_SEMANTIC_TOKEN_KEYS)[number], string>;

/** CSS custom properties a vintage style writes on :root (removed when leaving Vintage). */
export const VINTAGE_TOKEN_KEYS = [
  '--font-display',
  '--vintage-stripe-1',
  '--vintage-stripe-2',
  '--vintage-stripe-3',
  '--vintage-stripe-4',
  '--vintage-teal',
  '--vintage-paper-grain',
  '--vintage-paper-grain-size',
  '--vu-face',
  '--vu-ink',
  '--vu-hot',
  '--vu-ring',
] as const;

type VintageTokens = Record<(typeof VINTAGE_TOKEN_KEYS)[number], string>;

/** Surface styles a vintage style supplies to the shell, Home and player. */
export interface VintageSurfaceStyles {
  sidebar: React.CSSProperties;
  logoText: React.CSSProperties;
  navItem: React.CSSProperties;
  navItemActive: React.CSSProperties;
  navItemActiveCollapsed: React.CSSProperties;
  sectionLabel: React.CSSProperties;
  libraryItem: React.CSSProperties;
  libraryItemActive: React.CSSProperties;
  main: React.CSSProperties;
  homeCard: React.CSSProperties;
  homeCardTitle: React.CSSProperties;
  homeHeroTitle: React.CSSProperties;
  statTile: React.CSSProperties;
  statValue: React.CSSProperties;
  statLabel: React.CSSProperties;
  recentAlbumTile: React.CSSProperties;
  recentAlbumSleeve: React.CSSProperties;
  recentAlbumRecord: React.CSSProperties;
  recentAlbumRecordHovered: React.CSSProperties;
  recentAlbumLabel: React.CSSProperties;
  recentAlbumSpindle: React.CSSProperties;
  dockBar: React.CSSProperties;
  dockAlbumArtWrap: React.CSSProperties;
  dockCtrlBtn: React.CSSProperties;
  dockPlayBtn: React.CSSProperties;
  dockProgressFill: string;
}

/** Vintage Style Definition is part of this module's public API. */
export interface VintageStyleDefinition {
  id: VintageStyle;
  name: string;
  era: string;
  /** Overlaid on AppSettings by resolveHybridThemeSettings. */
  palette: {
    colorBg: string;
    colorSurface: string;
    colorBorder: string;
    colorAccent: string;
    colorText: string;
    colorTextMuted: string;
  };
  fontFamily: string;
  semanticTokens: SemanticTokens;
  tokens: VintageTokens;
  /** Colors of the decorative stripe band, top to bottom. */
  stripes: readonly string[];
  /** Offset-shadow colors cycled across stat tiles. */
  stickerShadows: readonly string[];
  /** CSS custom properties scoped to the player dock (a dark surface on a light theme). */
  dockVars: Record<string, string>;
  /** Colors for the Settings picker's mini preview. */
  preview: { bg: string; top: string; side: string; sideItem: string; accent: string; bar: string; meter: string };
  styles: VintageSurfaceStyles;
  loadFonts: () => Promise<unknown>;
}

const RECORD_SHOP_STRIPES = ['#d9a026', '#e07a2c', '#c4541f', '#7a3b1d'] as const;
const RECORD_SHOP_TEAL = '#2e6e6a';
const RECORD_SHOP_DISPLAY_FONT = "'Fraunces', Georgia, 'Times New Roman', serif";
const RECORD_SHOP_BODY_FONT =
  "'Karla', 'Satoshi', Aptos, \"Segoe UI Variable\", \"Segoe UI\", system-ui, sans-serif";

const RECORD_SHOP: VintageStyleDefinition = {
  id: 'recordshop',
  name: 'Record Shop ’74',
  era: 'Paper sleeves · light',
  palette: {
    colorBg: '#efe4cc',
    colorSurface: '#f7efdc',
    colorBorder: '#c9b58f',
    // Deep rust (≥4.5:1 on paper for links/text); the brighter #c4541f is stripe 3.
    colorAccent: '#a8441a',
    colorText: '#2b2118',
    colorTextMuted: '#6b5840',
  },
  fontFamily: RECORD_SHOP_BODY_FONT,
  semanticTokens: {
    '--surface-raised': '#fbf5e6',
    '--surface-subtle': '#e9dcc0',
    '--surface-hover': '#e4d5b6',
    '--divider-subtle': '#dccaa6',
    '--border-strong': '#b39d74',
    '--text-faint': '#7d6a50',
    '--accent-soft': '#f0d3bd',
    '--accent-secondary': '#d9a026',
    '--on-accent': '#fff8ea',
    '--focus': '#a8441a',
    '--focus-ring': '0 0 0 3px rgba(168,68,26,0.28)',
    '--success': RECORD_SHOP_TEAL,
    '--warning': '#9a641d',
    '--danger': '#b43d2d',
    '--overlay': 'rgba(43,33,24,0.5)',
    '--shadow-subtle': '0 2px 0 rgba(43,33,24,0.12)',
    '--shadow-raised': '0 18px 40px rgba(43,33,24,0.2)',
  },
  tokens: {
    '--font-display': RECORD_SHOP_DISPLAY_FONT,
    '--vintage-stripe-1': RECORD_SHOP_STRIPES[0],
    '--vintage-stripe-2': RECORD_SHOP_STRIPES[1],
    '--vintage-stripe-3': RECORD_SHOP_STRIPES[2],
    '--vintage-stripe-4': RECORD_SHOP_STRIPES[3],
    '--vintage-teal': RECORD_SHOP_TEAL,
    '--vintage-paper-grain': 'radial-gradient(rgba(90,60,20,0.07) 1px, transparent 1px)',
    '--vintage-paper-grain-size': '5px 5px',
    '--vu-face': '#f2c57a',
    '--vu-ink': '#2b2118',
    '--vu-hot': '#c4541f',
    '--vu-ring': '#d9a026',
  },
  stripes: RECORD_SHOP_STRIPES,
  stickerShadows: [RECORD_SHOP_STRIPES[0], RECORD_SHOP_STRIPES[1], RECORD_SHOP_STRIPES[2], RECORD_SHOP_TEAL, RECORD_SHOP_STRIPES[3]],
  dockVars: {
    '--bg': '#2b2118',
    '--surface': '#3a2c20',
    '--border': '#6b5840',
    '--accent': '#e07a2c',
    '--text': '#f7efdc',
    '--text-muted': '#d8c7a6',
    '--surface-raised': '#3f3024',
    '--surface-subtle': '#35281d',
    '--surface-hover': '#44342a',
    '--divider-subtle': '#4a3a2a',
    '--border-strong': '#7d6a50',
    '--text-faint': '#a8977a',
    '--accent-soft': '#4a2e1c',
    '--on-accent': '#2b2118',
  },
  preview: {
    bg: '#efe4cc',
    top: RECORD_SHOP_STRIPES[0],
    side: '#efe4cc',
    sideItem: '#c9b58f',
    accent: RECORD_SHOP_TEAL,
    bar: '#2b2118',
    meter: '#f2c57a',
  },
  styles: {
    sidebar: {
      backgroundColor: 'var(--bg)',
      backgroundImage: 'var(--vintage-paper-grain)',
      backgroundSize: 'var(--vintage-paper-grain-size)',
      borderRight: '1px solid var(--border)',
    },
    logoText: {
      fontFamily: 'var(--font-display)',
      fontStyle: 'italic',
      fontWeight: 800,
      letterSpacing: -0.5,
    },
    // Crate-divider tab: flat on the left, flush with the sidebar edge, rounded on the right.
    navItem: {
      marginLeft: -12,
      width: 'calc(100% + 12px)',
      paddingLeft: 26,
      borderRadius: '0 23px 23px 0',
      color: 'var(--text)',
      fontWeight: 600,
    },
    navItemActive: {
      backgroundColor: 'var(--vintage-teal)',
      color: 'var(--on-accent)',
      boxShadow: 'none',
      fontWeight: 700,
    },
    navItemActiveCollapsed: {
      backgroundColor: 'var(--vintage-teal)',
      color: 'var(--on-accent)',
      boxShadow: 'none',
    },
    sectionLabel: {
      fontFamily: 'var(--font-display)',
      fontWeight: 800,
      color: 'var(--vintage-stripe-4)',
      letterSpacing: 2,
    },
    libraryItem: {
      border: '2px solid var(--border)',
      borderRadius: 4,
      color: 'var(--text)',
      fontWeight: 600,
    },
    libraryItemActive: {
      backgroundColor: 'var(--vintage-stripe-1)',
      borderColor: 'var(--vintage-stripe-1)',
      color: 'var(--text)',
      fontWeight: 700,
    },
    main: {
      backgroundImage: 'var(--vintage-paper-grain)',
      backgroundSize: 'var(--vintage-paper-grain-size)',
    },
    homeCard: {
      background: 'var(--surface)',
      border: '1px solid var(--border)',
      borderRadius: 8,
      boxShadow: 'none',
    },
    homeCardTitle: {
      fontFamily: 'var(--font-display)',
      fontWeight: 800,
      fontSize: 22,
      letterSpacing: -0.3,
    },
    homeHeroTitle: {
      fontStyle: 'italic',
      fontSize: 30,
      letterSpacing: -0.8,
    },
    statTile: {
      backgroundColor: 'var(--surface-raised)',
      border: '2px solid var(--text)',
      borderRadius: 6,
    },
    statValue: {
      fontFamily: 'var(--font-display)',
      fontWeight: 800,
      color: 'var(--text)',
    },
    statLabel: {
      color: 'var(--text-muted)',
      fontWeight: 700,
    },
    recentAlbumTile: {
      overflow: 'visible',
    },
    recentAlbumSleeve: {
      overflow: 'visible',
      borderRadius: 2,
      border: 'none',
      boxShadow: 'none',
    },
    // Record peeking out of the right side of the sleeve (sleeve is 150px; disc is 92%).
    recentAlbumRecord: {
      position: 'absolute',
      top: '4%',
      left: '22%',
      width: '92%',
      height: '92%',
      borderRadius: '50%',
      background: 'repeating-radial-gradient(circle, #161310 0 2px, #221d18 2px 3px)',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      zIndex: 0,
      transition: 'transform 160ms ease-out',
    },
    recentAlbumRecordHovered: {
      transform: 'translateX(12px)',
    },
    // Centre label = a round crop of the same cover.
    recentAlbumLabel: {
      position: 'relative',
      width: '38%',
      height: '38%',
      borderRadius: '50%',
      overflow: 'hidden',
      boxShadow: '0 0 0 2px #161310',
      backgroundColor: 'var(--vintage-stripe-1)',
    },
    recentAlbumSpindle: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      width: 5,
      height: 5,
      margin: '-2.5px 0 0 -2.5px',
      borderRadius: '50%',
      backgroundColor: '#161310',
    },
    dockBar: {
      background: 'var(--bg)',
      backdropFilter: 'none',
      borderTop: 'none',
      boxShadow: 'inset 0 3px 0 var(--vintage-stripe-1), 0 -10px 28px rgba(43,33,24,0.18)',
    },
    dockAlbumArtWrap: {
      borderRadius: 2,
      border: 'none',
      overflow: 'visible',
      boxShadow: '3px 3px 0 var(--vintage-stripe-1)',
    },
    // 1px keeps the stacked side controls compact within the desktop dock.
    dockCtrlBtn: {
      background: 'transparent',
      border: '1px solid var(--border)',
      color: 'var(--text-muted)',
    },
    dockPlayBtn: {
      width: 50,
      height: 50,
      border: 'none',
      backgroundColor: 'var(--vintage-stripe-2)',
      color: 'var(--bg)',
      // Soft drop shadow only: a hard offset "sticker" shadow on a round button reads as a
      // second button peeking out from behind it.
      boxShadow: '0 2px 8px rgba(0,0,0,0.45)',
    },
    // Three hard-stop bands (mustard / orange / rust), not a colour wash.
    dockProgressFill: `linear-gradient(to bottom, ${RECORD_SHOP_STRIPES[0]} 0 34%, ${RECORD_SHOP_STRIPES[1]} 34% 67%, ${RECORD_SHOP_STRIPES[2]} 67% 100%)`,
  },
  // Bundled (no third-party font requests); loaded only when this style is active.
  loadFonts: () => Promise.all([
    import('@fontsource/fraunces/latin-800.css'),
    import('@fontsource/fraunces/latin-800-italic.css'),
    import('@fontsource/karla/latin-400.css'),
    import('@fontsource/karla/latin-600.css'),
    import('@fontsource/karla/latin-700.css'),
  ]),
};

/** VINTAGE STYLES is part of this module's public API. */
export const VINTAGE_STYLES: Record<VintageStyle, VintageStyleDefinition> = {
  recordshop: RECORD_SHOP,
};

const VINTAGE_STYLE_SET = new Set<string>(VINTAGE_STYLE_IDS);

/** Returns the style id when valid, otherwise null. */
export function parseVintageStyle(value: unknown): VintageStyle | null {
  return typeof value === 'string' && VINTAGE_STYLE_SET.has(value) ? value as VintageStyle : null;
}

/** Returns the style definition, falling back to the default style. */
export function getVintageStyle(style: VintageStyle | null | undefined): VintageStyleDefinition {
  return VINTAGE_STYLES[style ?? DEFAULT_VINTAGE_STYLE] ?? VINTAGE_STYLES[DEFAULT_VINTAGE_STYLE];
}

/**
 * Writes (or, with null, removes) the vintage tokens and the `data-vintage-style`
 * attribute on the document root. Always called with null when leaving Vintage so
 * nothing leaks into Light/Dark/Custom.
 */
export function applyVintageTokens(
  style: VintageStyleDefinition | null,
  doc: Document = document,
): void {
  const root = doc.documentElement;
  for (const key of VINTAGE_TOKEN_KEYS) {
    if (style) root.style.setProperty(key, style.tokens[key]);
    else root.style.removeProperty(key);
  }
  if (style) root.dataset.vintageStyle = style.id;
  else delete root.dataset.vintageStyle;
}

/**
 * The style's page palette as CSS custom properties. Popups rendered inside the
 * (dark-scoped) player dock apply these so they match the rest of the page.
 */
export function getVintagePageVars(style: VintageStyleDefinition): Record<string, string> {
  return {
    '--bg': style.palette.colorBg,
    '--surface': style.palette.colorSurface,
    '--border': style.palette.colorBorder,
    '--accent': style.palette.colorAccent,
    '--text': style.palette.colorText,
    '--text-muted': style.palette.colorTextMuted,
    ...style.semanticTokens,
  };
}

/** Loads the style's bundled fonts; failures fall back to the system stacks. */
export function mountVintageFonts(style: VintageStyleDefinition): Promise<void> {
  return style.loadFonts().then(() => undefined, () => undefined);
}

function channelToLinear(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function hexLuminance(hex: string): number {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) throw new Error(`Expected #rrggbb, got ${hex}`);
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 255, g = (n >> 8) & 255, b = n & 255;
  return 0.2126 * channelToLinear(r) + 0.7152 * channelToLinear(g) + 0.0722 * channelToLinear(b);
}

/** WCAG contrast ratio between two #rrggbb colors. */
export function contrastRatio(a: string, b: string): number {
  const la = hexLuminance(a), lb = hexLuminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

/** Active vintage style for the desktop shell, or null outside Vintage mode. */
export const VintageStyleContext = createContext<VintageStyleDefinition | null>(null);

/** Returns the active vintage style definition, or null when Vintage is not active. */
export function useVintageStyle(): VintageStyleDefinition | null {
  return useContext(VintageStyleContext);
}
