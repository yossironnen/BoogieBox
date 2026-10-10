// Shared single-row header for the browse screens (Browse root, Browse
// drill-down, Search): screen controls on the left, compact library totals,
// optional icon actions on the right, and a decorative screen-name watermark
// behind everything. Replaces the old full-width StatsBar + page hero stack.
// See wip/browse-command-bar-plan.md.
import React, { useRef } from 'react';
import type { Stats } from '../types';
import { useContainerSize } from '../hooks/useContainerSize';

type IconProps = { size?: number; style?: React.CSSProperties };

export function StatsTracksIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  );
}

export function StatsArtistsIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M20 21a8 8 0 0 0-16 0" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  );
}

export function StatsAlbumsIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.5" />
    </svg>
  );
}

export function StatsHoursIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l4 2" />
    </svg>
  );
}

export function StatsStorageIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <rect x="3" y="4" width="18" height="7" rx="1.5" />
      <rect x="3" y="13" width="18" height="7" rx="1.5" />
      <circle cx="7" cy="7.5" r="0.9" fill="currentColor" stroke="none" />
      <circle cx="7" cy="16.5" r="0.9" fill="currentColor" stroke="none" />
    </svg>
  );
}

export type StatsTier = 'full' | 'short' | 'hidden';

/** Strip tier for a measured bar width. Unmeasured (0) renders the full strip so
 * layout-less environments (jsdom tests) see every item. */
export function getStatsTier(barWidth: number): StatsTier {
  if (barWidth <= 0 || barWidth >= 1180) return 'full';
  if (barWidth >= 960) return 'short';
  return 'hidden';
}

type StatItem = { key: string; value: string; display: string; label: string; Icon: (props: IconProps) => React.ReactElement };

export function buildLibraryStatItems(stats: Stats | null | undefined): StatItem[] {
  const count = (value: number | undefined | null) => (value != null ? value.toLocaleString() : '–');
  const hours = stats?.total_hours != null ? stats.total_hours.toLocaleString(undefined, { maximumFractionDigits: 0 }) : '–';
  const gb = stats?.total_gb != null ? stats.total_gb.toLocaleString(undefined, { maximumFractionDigits: 1 }) : '–';
  const tracks = count(stats?.total_tracks);
  const artists = count(stats?.total_artists);
  const albums = count(stats?.total_albums);
  return [
    { key: 'tracks', value: tracks, display: tracks, label: 'tracks', Icon: StatsTracksIcon },
    { key: 'artists', value: artists, display: artists, label: 'artists', Icon: StatsArtistsIcon },
    { key: 'albums', value: albums, display: albums, label: 'albums', Icon: StatsAlbumsIcon },
    { key: 'hours', value: hours, display: hours === '–' ? hours : `${hours}h`, label: 'hours', Icon: StatsHoursIcon },
    { key: 'gb', value: gb, display: gb === '–' ? gb : `${gb} GB`, label: 'GB', Icon: StatsStorageIcon },
  ];
}

const SHORT_TIER_KEYS = new Set(['tracks', 'albums']);

export function formatLibraryStatsTitle(stats: Stats | null | undefined): string {
  return `Library totals: ${buildLibraryStatItems(stats).map((item) => `${item.value} ${item.label}`).join(', ')}`;
}

export function LibraryStatsStrip({ stats, tier = 'full' }: { stats: Stats | null | undefined; tier?: StatsTier }) {
  if (tier === 'hidden') return null;
  const items = buildLibraryStatItems(stats).filter((item) => tier === 'full' || SHORT_TIER_KEYS.has(item.key));
  return (
    <div style={styles.strip} title={formatLibraryStatsTitle(stats)} data-testid="library-stats-strip">
      {items.map(({ key, value, display, label, Icon }) => (
        <span key={key} style={styles.stat} aria-label={`${value} ${label}`}>
          <Icon size={13} style={styles.statIcon} />
          <span aria-hidden="true">{display}</span>
        </span>
      ))}
    </div>
  );
}

export interface CommandBarProps {
  /** Screen name drawn as the decorative watermark ("Browse", "Search"). */
  watermark: string;
  stats: Stats | null | undefined;
  /** Left zone: tabs, chips, breadcrumb, search input… */
  children: React.ReactNode;
  /** Right zone after the divider: icon buttons. */
  actions?: React.ReactNode;
  style?: React.CSSProperties;
}

export default function CommandBar({ watermark, stats, children, actions, style }: CommandBarProps) {
  const barRef = useRef<HTMLDivElement | null>(null);
  const { size, attachRef } = useContainerSize(barRef);
  const tier = getStatsTier(size.width);
  return (
    <div
      ref={attachRef}
      style={{ ...styles.bar, ...style }}
      title={tier === 'hidden' ? formatLibraryStatsTitle(stats) : undefined}
      data-command-bar={watermark.toLowerCase()}
    >
      <div aria-hidden="true" style={styles.watermarkLayer}>
        <span style={styles.watermark} data-testid="command-bar-watermark">{watermark}</span>
      </div>
      <div style={styles.left}>{children}</div>
      <div style={styles.layer}>
        <LibraryStatsStrip stats={stats} tier={tier} />
      </div>
      {actions && (
        <div style={styles.actions}>
          <span style={styles.divider} />
          {actions}
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  bar: {
    position: 'relative',
    // `isolation` makes the bar its own stacking context, so popovers anchored
    // inside it can only rise above the content below if the bar itself does.
    isolation: 'isolate',
    zIndex: 30,
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'nowrap',
    gap: 10,
    minHeight: 56,
    padding: '8px 20px',
    boxSizing: 'border-box',
    flexShrink: 0,
    borderBottom: '1px solid color-mix(in srgb, var(--border) 62%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 26%, transparent)',
  },
  // Clipping lives on this layer, not on the bar: the bar must stay
  // overflow-visible so popovers anchored inside it are not cut off.
  watermarkLayer: {
    position: 'absolute',
    inset: 0,
    overflow: 'hidden',
    pointerEvents: 'none',
    zIndex: 0,
  },
  watermark: {
    position: 'absolute',
    left: '50%',
    top: '50%',
    transform: 'translate(-50%, -50%)',
    // Cap height (~70px) is taller than the 56px bar, so the layer trims the
    // word top and bottom on purpose.
    fontSize: 100,
    fontWeight: 800,
    lineHeight: 1,
    letterSpacing: -3.5,
    whiteSpace: 'nowrap',
    color: 'color-mix(in srgb, var(--accent) 35%, var(--text))',
    opacity: 0.12,
    userSelect: 'none',
  },
  left: {
    position: 'relative',
    zIndex: 1,
    flex: '1 1 auto',
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 10,
  },
  layer: {
    position: 'relative',
    zIndex: 1,
    flexShrink: 0,
  },
  actions: {
    position: 'relative',
    zIndex: 1,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  },
  divider: {
    width: 1,
    height: 22,
    background: 'var(--border)',
    margin: '0 4px',
  },
  strip: {
    display: 'flex',
    alignItems: 'center',
    gap: 14,
    color: 'var(--text-muted)',
    fontSize: 13,
    fontWeight: 500,
    fontVariantNumeric: 'tabular-nums',
    whiteSpace: 'nowrap',
  },
  stat: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 5,
  },
  statIcon: {
    color: 'var(--accent)',
    opacity: 0.85,
    flexShrink: 0,
  },
};
