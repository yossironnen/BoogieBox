/**
 * Defines Ui Phase2 behavior for BoogieBox.
 */

import type React from 'react';
import { MOBILE_CONTENT_DOCK_CLEARANCE } from './hybridPreview';

export const phase2 = {
  desktopHero: {
    position: 'relative' as const,
    overflow: 'hidden' as const,
    borderBottom: '1px solid color-mix(in srgb, var(--border) 62%, transparent)',
    background: [
      'radial-gradient(circle at top left, color-mix(in srgb, var(--accent) 22%, transparent) 0%, transparent 42%)',
      'linear-gradient(135deg, color-mix(in srgb, var(--surface) 90%, var(--bg)) 0%, color-mix(in srgb, var(--surface) 58%, var(--bg)) 100%)',
    ].join(','),
    boxShadow: 'inset 0 -1px 0 rgba(255,255,255,0.02)',
  },
  desktopHeroInner: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 20,
    padding: '22px 24px 18px',
    flexWrap: 'wrap' as const,
  },
  eyebrow: {
    fontSize: 13,
    letterSpacing: 1.1,
    textTransform: 'uppercase' as const,
    color: 'var(--text-muted)',
    fontWeight: 700,
  },
  heroTitle: {
    marginTop: 8,
    fontSize: 32,
    lineHeight: 1.02,
    letterSpacing: -0.9,
    color: 'var(--text)',
    fontWeight: 800,
  },
  heroBody: {
    marginTop: 10,
    fontSize: 16,
    lineHeight: 1.55,
    color: 'color-mix(in srgb, var(--text) 88%, var(--text-muted))',
    maxWidth: 720,
  },
  tray: {
    border: '1px solid color-mix(in srgb, var(--border) 74%, transparent)',
    borderRadius: 18,
    background: 'color-mix(in srgb, var(--surface) 84%, var(--bg))',
    boxShadow: '0 18px 36px rgba(0,0,0,0.16)',
  },
  desktopMediaRow: {
    border: '1px solid color-mix(in srgb, var(--border) 60%, transparent)',
    borderRadius: 18,
    background: 'color-mix(in srgb, var(--surface) 72%, var(--bg))',
    boxShadow: '0 10px 24px rgba(0,0,0,0.1)',
  },
  mobilePage: {
    minHeight: '100%',
    boxSizing: 'border-box' as const,
    padding: `18px 16px calc(env(safe-area-inset-bottom, 0px) + ${MOBILE_CONTENT_DOCK_CLEARANCE}px)`,
    background: 'transparent',
  },
  mobileHeroCard: {
    position: 'relative' as const,
    overflow: 'hidden' as const,
    borderRadius: 28,
    border: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
    background: [
      'radial-gradient(circle at top left, color-mix(in srgb, var(--accent) 34%, transparent) 0%, transparent 40%)',
      'linear-gradient(160deg, color-mix(in srgb, var(--surface) 96%, var(--bg)) 0%, color-mix(in srgb, var(--surface) 72%, var(--bg)) 100%)',
    ].join(','),
    boxShadow: '0 20px 44px rgba(0,0,0,0.22)',
  },
  mobileCard: {
    borderRadius: 24,
    border: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 86%, var(--bg))',
    boxShadow: '0 14px 28px rgba(0,0,0,0.12)',
  },
  mobileTitle: {
    color: 'var(--text)',
    fontSize: 32,
    fontWeight: 800,
    lineHeight: 1.02,
    letterSpacing: -0.9,
  },
  mobileKicker: {
    color: 'var(--text-muted)',
    fontSize: 13,
    fontWeight: 700,
    letterSpacing: 1.1,
    textTransform: 'uppercase' as const,
  },
  mobileMediaRow: {
    borderRadius: 22,
    border: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 88%, var(--bg))',
    boxShadow: '0 12px 28px rgba(0,0,0,0.12)',
  },
};
