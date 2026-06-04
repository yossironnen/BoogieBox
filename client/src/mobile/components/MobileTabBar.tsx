/**
 * Defines mobile Mobile Tab Bar behavior for the BoogieBox React client.
 */

import React from 'react';
import type { MobileTabId } from '../mobileShell';

const TABS: Array<{ id: MobileTabId; label: string; icon: string }> = [
  { id: 'home', label: 'Home', icon: '⌂' },
  { id: 'browse', label: 'Browse', icon: '♪' },
  { id: 'search', label: 'Search', icon: '⌕' },
  { id: 'playlists', label: 'Playlists', icon: '☰' },
  { id: 'now-playing', label: 'Now', icon: '▶' },
];

/** Mobile Tab Bar is part of this module's public API. */
export default function MobileTabBar({
  activeTab,
  onChange,
}: {
  activeTab: MobileTabId;
  onChange: (tab: MobileTabId) => void;
}) {
  return (
    <nav style={styles.bar} aria-label="Mobile tabs">
      {TABS.map((tab) => {
        const active = tab.id === activeTab;
        return (
          <button
            key={tab.id}
            type="button"
            aria-current={active ? 'page' : undefined}
            style={{ ...styles.tab, ...(active ? styles.tabActive : null) }}
            onClick={() => onChange(tab.id)}
          >
            <span style={styles.icon} aria-hidden="true">{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        );
      })}
    </nav>
  );
}

const styles: Record<string, React.CSSProperties> = {
  bar: {
    position: 'fixed',
    left: 0,
    right: 0,
    bottom: 0,
    zIndex: 50,
    display: 'grid',
    gridTemplateColumns: 'repeat(5, 1fr)',
    gap: 4,
    padding: '8px 8px calc(env(safe-area-inset-bottom, 0px) + 10px)',
    background: 'rgba(12, 12, 16, 0.96)',
    backdropFilter: 'blur(20px)',
    borderTop: '1px solid color-mix(in srgb, var(--border) 85%, transparent)',
  },
  tab: {
    minHeight: 54,
    border: 'none',
    borderRadius: 14,
    background: 'transparent',
    color: 'var(--text-muted)',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 4,
    fontFamily: 'inherit',
    fontSize: 10,
    fontWeight: 600,
    cursor: 'pointer',
  },
  tabActive: {
    color: 'var(--text)',
    background: 'color-mix(in srgb, var(--accent) 18%, rgba(255,255,255,0.02))',
    boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--accent) 28%, transparent)',
  },
  icon: {
    fontSize: 18,
    lineHeight: 1,
  },
};
