/**
 * Vintage theme registry: palettes, tokens, fonts and surface styles for the
 * desktop "Vintage" theme mode (Record Shop '74, Hi-Fi Console '63). Further
 * styles are added as one more entry in VINTAGE_STYLES (plus the matching id in
 * the server's UI_VINTAGE_STYLES allowlist).
 */

import { createContext, useContext } from 'react';
import type React from 'react';
import type { HYBRID_SEMANTIC_TOKEN_KEYS } from './hybridPreview';

/** Vintage Style is part of this module's public API. */
export type VintageStyle = 'recordshop' | 'hificonsole';

/** Vintage style ids in picker order. */
export const VINTAGE_STYLE_IDS: readonly VintageStyle[] = ['recordshop', 'hificonsole'];

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
  // Active sidebar tab background (teal in Record Shop; the name predates other styles).
  '--vintage-teal',
  '--vintage-paper-grain',
  '--vintage-paper-grain-size',
  '--vu-face',
  '--vu-ink',
  '--vu-hot',
  '--vu-ring',
  '--vintage-knob',
  '--vintage-knob-pointer',
  '--vintage-knob-caption',
] as const;

type VintageTokens = Record<(typeof VINTAGE_TOKEN_KEYS)[number], string>;

/** Seek-bar look in the player dock. */
export interface VintageProgressStyle {
  /** CSS background of the filled part. */
  fill: string;
  trackHeight: number;
  track?: React.CSSProperties;
  thumb?: React.CSSProperties;
}

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
  /** Home "Recently Added" row: room for the record peeking out of each sleeve. */
  recentAlbumRow: React.CSSProperties;
  recentAlbumTile: React.CSSProperties;
  recentAlbumSleeve: React.CSSProperties;
  recentAlbumRecord: React.CSSProperties;
  recentAlbumRecordHovered: React.CSSProperties;
  recentAlbumLabel: React.CSSProperties;
  recentAlbumSpindle: React.CSSProperties;
  /** Cover in front of the record (Home and Browse). */
  sleeveCover: React.CSSProperties;
  /** Browse grid: sleeve position inside the unchanged art box. */
  gridSleeve: React.CSSProperties;
  /** Browse grid: smaller hover slide than Home. */
  gridRecordHovered: React.CSSProperties;
  dockBar: React.CSSProperties;
  dockAlbumArtWrap: React.CSSProperties;
  dockCtrlBtn: React.CSSProperties;
  dockPlayBtn: React.CSSProperties;
  dockProgress: VintageProgressStyle;
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
  /** Offset-shadow colors cycled across stat tiles (empty = no sticker shadow). */
  stickerShadows: readonly string[];
  /** CSS custom properties scoped to the player dock (a dark surface on a light theme). */
  dockVars: Record<string, string>;
  /** Colors for the Settings picker's mini preview. */
  preview: {
    bg: string; top: string; side: string; sideItem: string; accent: string; bar: string; meter: string;
    /** Content tiles; defaults to stripes + sticker colors. */
    tiles?: readonly string[];
  };
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
    '--vintage-knob': 'radial-gradient(circle at 35% 30%, #4a3a2e 0, #1c140f 60%, #0a0705 100%)',
    '--vintage-knob-pointer': '#d9a026',
    '--vintage-knob-caption': '#d9a026',
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
    recentAlbumRow: { gap: 40, paddingRight: 36, paddingTop: 2 },
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
    sleeveCover: { boxShadow: '3px 3px 0 rgba(43,33,24,0.25)' },
    // Sleeve at 86% of the art box, so the record peeks into the remaining width.
    gridSleeve: { left: 0, top: '7%', width: '86%' },
    gridRecordHovered: { transform: 'translateX(8px)' },
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
    dockProgress: {
      fill: `linear-gradient(to bottom, ${RECORD_SHOP_STRIPES[0]} 0 34%, ${RECORD_SHOP_STRIPES[1]} 34% 67%, ${RECORD_SHOP_STRIPES[2]} 67% 100%)`,
      trackHeight: 8,
    },
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

// Brass trim band, light to dark (reads as a bevelled strip along the top).
const HIFI_TRIM = ['#e6c97a', '#c9a24a', '#6e521f'] as const;
const HIFI_BRASS = '#c9a24a';
const HIFI_BRASS_DARK = '#8a6a2a';
const HIFI_WALNUT = '#2a1a10';
const HIFI_LAMP = '#ffb347';
const HIFI_DIAL_RED = '#d2321e';
const HIFI_PANEL = '#0a0c0e';
const HIFI_DISPLAY_FONT = "'Bodoni Moda', Didot, Georgia, 'Times New Roman', serif";
const HIFI_BODY_FONT =
  "'Jost', Futura, 'Century Gothic', Aptos, \"Segoe UI Variable\", \"Segoe UI\", system-ui, sans-serif";
const HIFI_GROOVES = 'repeating-radial-gradient(circle, #141414 0 2px, #222 2px 3px)';
// Pilot lamp drawn into the nav button's background, left of the icon.
const HIFI_LAMP_OFF = 'radial-gradient(circle at 15px 50%, #4a2f1c 0 3.5px, transparent 4px)';
const HIFI_LAMP_ON = `radial-gradient(circle at 15px 50%, ${HIFI_LAMP} 0 3.5px, rgba(255,170,60,0.5) 4.5px, transparent 9px)`;
const HIFI_PRESSED_BUTTON = {
  backgroundColor: 'var(--vintage-teal)',
  color: 'var(--text)',
  border: '1px solid #000',
  boxShadow: 'inset 0 2px 6px rgba(0,0,0,0.8), 0 1px 0 rgba(255,220,160,0.12)',
} as const;

const HIFI_CONSOLE: VintageStyleDefinition = {
  id: 'hificonsole',
  name: 'Hi-Fi Console ’63',
  era: 'Walnut & brass · dark',
  palette: {
    colorBg: '#121417',
    colorSurface: '#1a1d21',
    colorBorder: '#3b3830',
    colorAccent: HIFI_BRASS,
    colorText: '#efe6d2',
    colorTextMuted: '#a69c88',
  },
  fontFamily: HIFI_BODY_FONT,
  semanticTokens: {
    '--surface-raised': '#20242a',
    '--surface-subtle': '#16191c',
    '--surface-hover': '#262a30',
    '--divider-subtle': '#2a2e33',
    '--border-strong': '#5a5345',
    '--text-faint': '#867c69',
    '--accent-soft': '#3a3020',
    '--accent-secondary': HIFI_LAMP,
    '--on-accent': '#1a1205',
    '--focus': '#e6c97a',
    '--focus-ring': '0 0 0 3px rgba(201,162,74,0.35)',
    '--success': '#86b87a',
    '--warning': HIFI_LAMP,
    '--danger': '#e0604a',
    '--overlay': 'rgba(0,0,0,0.6)',
    '--shadow-subtle': '0 1px 0 rgba(0,0,0,0.5)',
    '--shadow-raised': '0 18px 40px rgba(0,0,0,0.55)',
  },
  tokens: {
    '--font-display': HIFI_DISPLAY_FONT,
    '--vintage-stripe-1': HIFI_TRIM[0],
    '--vintage-stripe-2': HIFI_TRIM[1],
    '--vintage-stripe-3': HIFI_TRIM[2],
    '--vintage-stripe-4': HIFI_WALNUT,
    '--vintage-teal': '#0d0f11',
    // Faint horizontal lines across the smoked-glass faceplate.
    '--vintage-paper-grain': 'repeating-linear-gradient(0deg, rgba(255,255,255,0.012) 0 1px, transparent 1px 3px)',
    '--vintage-paper-grain-size': 'auto',
    '--vu-face': '#7cc4f0',
    '--vu-ink': '#0b1d2e',
    '--vu-hot': '#d23b2a',
    '--vu-ring': HIFI_BRASS,
    '--vintage-knob': 'conic-gradient(from 20deg, #f6f6f6, #9a9c9f, #f0f0f0, #8d8f92, #f6f6f6, #a2a4a7, #f6f6f6)',
    '--vintage-knob-pointer': HIFI_DIAL_RED,
    '--vintage-knob-caption': '#2b2b2b',
  },
  stripes: HIFI_TRIM,
  stickerShadows: [],
  // Brushed-aluminium receiver faceplate.
  dockVars: {
    '--bg': '#c9cacb',
    '--surface': '#d6d7d8',
    '--border': '#8e9093',
    '--accent': '#8f2414',
    '--text': '#161616',
    '--text-muted': '#3a3a3a',
    '--surface-raised': '#dfe0e1',
    '--surface-subtle': '#bfc1c3',
    '--surface-hover': '#b4b6b8',
    '--divider-subtle': '#a9abad',
    '--border-strong': '#6f7174',
    '--text-faint': '#4f5154',
    '--accent-soft': '#e3c2b8',
    '--on-accent': '#ffffff',
  },
  preview: {
    bg: '#121417',
    top: HIFI_BRASS,
    side: HIFI_WALNUT,
    sideItem: '#4a3524',
    accent: HIFI_LAMP,
    bar: '#c9cacb',
    meter: '#7cc4f0',
    tiles: ['#1c3b4a', HIFI_BRASS, '#2d4a3a', '#7a2e1f', '#6b8fa8', HIFI_LAMP],
  },
  styles: {
    sidebar: {
      backgroundColor: HIFI_WALNUT,
      backgroundImage: 'repeating-linear-gradient(91deg, rgba(0,0,0,0.18) 0 2px, rgba(255,210,150,0.04) 2px 5px, rgba(0,0,0,0.08) 5px 11px, rgba(255,200,140,0.03) 11px 13px)',
      borderRight: `3px solid ${HIFI_BRASS_DARK}`,
    },
    logoText: {
      fontFamily: 'var(--font-display)',
      fontStyle: 'italic',
      fontWeight: 700,
      color: '#e6c97a',
      letterSpacing: -0.3,
    },
    navItem: {
      paddingLeft: 30,
      borderRadius: 6,
      border: '1px solid transparent',
      backgroundImage: HIFI_LAMP_OFF,
      color: '#cfc3a9',
      fontSize: 13,
      fontWeight: 500,
      letterSpacing: 1.6,
      textTransform: 'uppercase',
    },
    navItemActive: {
      ...HIFI_PRESSED_BUTTON,
      backgroundImage: `${HIFI_LAMP_ON}, linear-gradient(#0b0c0e, #16181b)`,
    },
    // Icon-only: the lit icon stands in for the pilot lamp.
    navItemActiveCollapsed: {
      ...HIFI_PRESSED_BUTTON,
      color: HIFI_LAMP,
    },
    sectionLabel: {
      color: HIFI_BRASS,
      fontWeight: 600,
      letterSpacing: 3,
    },
    libraryItem: {
      border: '1px solid rgba(201,162,74,0.25)',
      borderRadius: 4,
      color: '#cfc3a9',
    },
    libraryItemActive: {
      backgroundColor: 'rgba(201,162,74,0.12)',
      borderColor: HIFI_BRASS,
      color: '#f3dfa6',
      fontWeight: 500,
    },
    main: {
      backgroundImage: 'radial-gradient(ellipse at 50% -10%, rgba(120,150,170,0.10), transparent 60%), var(--vintage-paper-grain)',
    },
    homeCard: {
      background: 'linear-gradient(#191c20, #0f1113)',
      border: '1px solid #2a2e33',
      borderRadius: 10,
      boxShadow: `inset 0 1px 0 rgba(255,255,255,0.05), 0 0 0 1px rgba(201,162,74,0.3)`,
    },
    homeCardTitle: {
      fontFamily: 'var(--font-display)',
      fontWeight: 600,
      fontSize: 22,
    },
    homeHeroTitle: {
      fontStyle: 'italic',
      fontWeight: 700,
      fontSize: 34,
      color: '#e6c97a',
      letterSpacing: -0.5,
    },
    // Recessed meter window with lit numerals.
    statTile: {
      backgroundColor: HIFI_PANEL,
      border: '1px solid #2a2e33',
      borderRadius: 8,
      boxShadow: 'inset 0 3px 10px rgba(0,0,0,0.9), 0 1px 0 rgba(255,255,255,0.05)',
    },
    statValue: {
      fontWeight: 300,
      color: HIFI_LAMP,
      textShadow: '0 0 10px rgba(255,150,40,0.55)',
    },
    statLabel: {
      color: 'var(--text-muted)',
      fontWeight: 500,
      letterSpacing: 2.5,
    },
    // The record rises out of the top of the sleeve, like a slot-loading changer.
    recentAlbumRow: { gap: 24, paddingTop: 46 },
    recentAlbumTile: {
      overflow: 'visible',
    },
    recentAlbumSleeve: {
      overflow: 'visible',
      borderRadius: 3,
      border: 'none',
      boxShadow: 'none',
    },
    recentAlbumRecord: {
      position: 'absolute',
      top: '-22%',
      left: '9%',
      width: '82%',
      height: '82%',
      borderRadius: '50%',
      background: HIFI_GROOVES,
      boxShadow: '0 0 0 2px #0a0a0a, inset 0 0 0 3px rgba(255,255,255,0.06)',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      zIndex: 0,
      transition: 'transform 160ms ease-out',
    },
    recentAlbumRecordHovered: {
      transform: 'translateY(-10px)',
    },
    recentAlbumLabel: {
      position: 'relative',
      width: '38%',
      height: '38%',
      borderRadius: '50%',
      overflow: 'hidden',
      boxShadow: `0 0 0 2px ${HIFI_BRASS}`,
      backgroundColor: HIFI_BRASS,
    },
    recentAlbumSpindle: {
      position: 'absolute',
      left: '50%',
      top: '50%',
      width: 5,
      height: 5,
      margin: '-2.5px 0 0 -2.5px',
      borderRadius: '50%',
      backgroundColor: '#0a0a0a',
    },
    // Brass bezel.
    sleeveCover: { boxShadow: `0 0 0 2px #0a0b0c, 0 0 0 3px ${HIFI_BRASS_DARK}, 0 10px 20px rgba(0,0,0,0.6)` },
    // Sleeve sits at the bottom of the art box; the record rises into the 18% above it.
    gridSleeve: { left: '9%', top: '18%', width: '82%' },
    gridRecordHovered: { transform: 'translateY(-6px)' },
    dockBar: {
      background: 'repeating-linear-gradient(90deg, rgba(255,255,255,0.10) 0 1px, rgba(0,0,0,0.03) 1px 3px), linear-gradient(#dedfe0, #b4b6b8)',
      backdropFilter: 'none',
      borderTop: `3px solid ${HIFI_BRASS_DARK}`,
      boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.8), 0 -8px 24px rgba(0,0,0,0.5)',
    },
    dockAlbumArtWrap: {
      borderRadius: 3,
      border: 'none',
      boxShadow: `0 0 0 2px #2a2a2a, 0 0 0 3px ${HIFI_BRASS}`,
    },
    // Chrome push buttons.
    dockCtrlBtn: {
      background: 'linear-gradient(#f2f3f4, #c3c5c7)',
      border: '1px solid #7d7f82',
      color: 'var(--text)',
      boxShadow: '0 1px 2px rgba(0,0,0,0.3)',
    },
    // Same chrome push button as prev/next, just larger.
    dockPlayBtn: {
      width: 46,
      height: 46,
      border: '1px solid #7d7f82',
      backgroundColor: '#d6d7d8',
      backgroundImage: 'linear-gradient(#f2f3f4, #c3c5c7)',
      color: 'var(--text)',
      boxShadow: '0 1px 2px rgba(0,0,0,0.3)',
    },
    // FM tuning dial: cream scale with tick marks, a red pointer as the playhead.
    dockProgress: {
      fill: 'transparent',
      trackHeight: 16,
      track: {
        background: [
          'repeating-linear-gradient(90deg, rgba(42,33,24,0.85) 0 1px, transparent 1px 10%) top / 100% 7px no-repeat',
          'repeating-linear-gradient(90deg, rgba(42,33,24,0.45) 0 1px, transparent 1px 2%) top / 100% 4px no-repeat',
          'linear-gradient(#f4ecd2, #e6dab6)',
        ].join(', '),
        borderRadius: 3,
        boxShadow: 'inset 0 1px 3px rgba(0,0,0,0.35), 0 0 0 1px #2a2a2a',
      },
      thumb: {
        width: 3,
        height: 22,
        borderRadius: 1,
        backgroundColor: HIFI_DIAL_RED,
        boxShadow: '0 0 6px rgba(230,60,30,0.8)',
      },
    },
  },
  loadFonts: () => Promise.all([
    import('@fontsource/bodoni-moda/latin-600.css'),
    import('@fontsource/bodoni-moda/latin-700-italic.css'),
    import('@fontsource/jost/latin-300.css'),
    import('@fontsource/jost/latin-400.css'),
    import('@fontsource/jost/latin-500.css'),
    import('@fontsource/jost/latin-600.css'),
  ]),
};

/** VINTAGE STYLES is part of this module's public API. */
export const VINTAGE_STYLES: Record<VintageStyle, VintageStyleDefinition> = {
  recordshop: RECORD_SHOP,
  hificonsole: HIFI_CONSOLE,
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
