/**
 * Unified desktop-sidebar identity, library totals and server activity panel.
 */

import React, { useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { AuthUser, BoogieMixDeepAnalysisStatus, Library, ScanJob, Stats } from '../types';
import { activeCount, type ActivitySnapshot } from '../activity/activityTypes';
import { ActivityIcon, DownloadIcon, MixJobIcon, PlayIcon } from './activityIcons';

interface Props {
  currentUser: AuthUser;
  collapsed: boolean;
  streamDirect: boolean;
  ffmpegAvailable: boolean | null;
  transcodeQuality: string;
  activeScanJobs: ScanJob[];
  libraries: Library[];
  deepAnalysisStatus: BoogieMixDeepAnalysisStatus | null;
  /** Library totals: a quiet line when expanded, an indicator popover when collapsed. */
  stats?: Stats | null;
  onLogout: () => void;
  /** Admins only: the live Activity snapshot; the indicator is hidden without it. */
  activity?: ActivitySnapshot | null;
  /** Admins only: opens the Activity page. */
  onOpenActivity?: () => void;
}

type IndicatorKind = 'transcoding' | 'scan' | 'deep-analysis' | 'library' | 'activity';

type IconProps = { size?: number; style?: React.CSSProperties };

/** Space between an indicator and its tooltip, and between a tooltip and the window edge. */
const TOOLTIP_GAP = 8;
const TOOLTIP_MARGIN = 8;

function StatsTracksIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  );
}

function StatsArtistsIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <path d="M20 21a8 8 0 0 0-16 0" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  );
}

function StatsAlbumsIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.5" />
    </svg>
  );
}

function StatsHoursIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l4 2" />
    </svg>
  );
}

function StatsStorageIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      <rect x="3" y="4" width="18" height="7" rx="1.5" />
      <rect x="3" y="13" width="18" height="7" rx="1.5" />
      <circle cx="7" cy="7.5" r="0.9" fill="currentColor" stroke="none" />
      <circle cx="7" cy="16.5" r="0.9" fill="currentColor" stroke="none" />
    </svg>
  );
}

function LibraryIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 4v16M9 4v16" />
      <path d="M13 5.2l3.9-1 4.1 15.6-3.9 1z" />
    </svg>
  );
}

type StatItem = {
  key: string;
  value: string;
  display: string;
  label: string;
  name: string;
  Icon: (props: IconProps) => React.ReactElement;
};

export function buildLibraryStatItems(stats: Stats | null | undefined): StatItem[] {
  const count = (value: number | undefined | null) => (value != null ? value.toLocaleString() : '–');
  const hours = stats?.total_hours != null ? stats.total_hours.toLocaleString(undefined, { maximumFractionDigits: 0 }) : '–';
  const gb = stats?.total_gb != null ? stats.total_gb.toLocaleString(undefined, { maximumFractionDigits: 1 }) : '–';
  const tracks = count(stats?.total_tracks);
  const artists = count(stats?.total_artists);
  const albums = count(stats?.total_albums);
  return [
    { key: 'tracks', value: tracks, display: tracks, label: 'tracks', name: 'Tracks', Icon: StatsTracksIcon },
    { key: 'artists', value: artists, display: artists, label: 'artists', name: 'Artists', Icon: StatsArtistsIcon },
    { key: 'albums', value: albums, display: albums, label: 'albums', name: 'Albums', Icon: StatsAlbumsIcon },
    { key: 'hours', value: hours, display: hours === '–' ? hours : `${hours}h`, label: 'hours', name: 'Playtime', Icon: StatsHoursIcon },
    { key: 'gb', value: gb, display: gb === '–' ? gb : `${gb} GB`, label: 'GB', name: 'Storage', Icon: StatsStorageIcon },
  ];
}

export function formatLibraryStatsTitle(stats: Stats | null | undefined): string {
  return `Library totals: ${buildLibraryStatItems(stats).map((item) => `${item.value} ${item.label}`).join(', ')}`;
}

function LibraryStatsLine({ stats }: { stats: Stats | null | undefined }) {
  const title = formatLibraryStatsTitle(stats);
  return (
    <div style={styles.statsLine} title={title} role="group" aria-label={title} data-testid="sidebar-library-stats">
      {buildLibraryStatItems(stats).map(({ key, value, display, label, Icon }) => (
        <span
          key={key}
          // Storage sits alone on the last row: span both columns and centre it.
          style={key === 'gb' ? { ...styles.statsLineItem, gridColumn: '1 / -1', justifySelf: 'center' } : styles.statsLineItem}
          aria-label={`${value} ${label}`}
        >
          <Icon size={12} style={styles.statIcon} />
          <span aria-hidden="true">{display}</span>
        </span>
      ))}
    </div>
  );
}

/** Up to two names, then `+N`. */
function shortList(names: string[]): string {
  const unique = [...new Set(names.filter(Boolean))];
  if (unique.length <= 2) return unique.join(', ');
  return `${unique.slice(0, 2).join(', ')} +${unique.length - 2}`;
}

export function describeActivity(activity: ActivitySnapshot | null | undefined): string {
  return `Activity · ${activeCount(activity?.totals)} active`;
}

function ActivitySummaryPopover({ activity }: { activity: ActivitySnapshot }) {
  const { totals } = activity;
  const playingNames = shortList(activity.playing.map((p) => [p.user, p.client].filter(Boolean).join(' · ')));
  const downloadNames = shortList(activity.downloads.map((d) => [d.user, d.client].filter(Boolean).join(' · ')));
  const mixNames = shortList(activity.mixJobs.map((j) => j.user ?? ''));
  const rows = [
    { key: 'play', Icon: PlayIcon, text: `${totals.playing} playing`, names: playingNames },
    { key: 'dl', Icon: DownloadIcon, text: `${totals.downloading} downloading`, names: downloadNames },
    { key: 'mix', Icon: MixJobIcon, text: `${totals.mixJobs} BoogieMix jobs`, names: mixNames },
  ];
  return (
    <span style={styles.statsPopover}>
      <span style={styles.statsPopoverTitle}>{describeActivity(activity)}</span>
      {rows.map(({ key, Icon, text, names }) => (
        <span key={key} style={styles.statsPopoverRow}>
          <Icon size={14} style={styles.statIcon} />
          <span style={styles.statsPopoverName}>{text}</span>
          <span style={{ ...styles.statsPopoverValue, fontWeight: 500 }}>{names}</span>
        </span>
      ))}
      <span style={{ ...styles.statsPopoverName, opacity: 0.8 }}>Open Activity</span>
    </span>
  );
}

function LibraryStatsPopover({ stats }: { stats: Stats | null | undefined }) {
  return (
    <span style={styles.statsPopover}>
      <span style={styles.statsPopoverTitle}>Library totals</span>
      {buildLibraryStatItems(stats).map(({ key, display, name, Icon }) => (
        <span key={key} style={styles.statsPopoverRow}>
          <Icon size={14} style={styles.statIcon} />
          <span style={styles.statsPopoverName}>{name}</span>
          <span style={styles.statsPopoverValue}>{display}</span>
        </span>
      ))}
    </span>
  );
}

export function describeTranscoding(
  streamDirect: boolean,
  ffmpegAvailable: boolean | null,
  transcodeQuality: string,
): string {
  if (streamDirect) return 'Transcoding off';
  if (ffmpegAvailable === null) return 'Server-side transcoding';
  if (!ffmpegAvailable) return 'No ffmpeg';
  return `Transcoding on (${transcodeQuality === 'high' ? '320 kbps' : '192 kbps'})`;
}

export function describeScan(activeScanJobs: ScanJob[], libraries: Library[]): string {
  const job = activeScanJobs.find((entry) => entry.status === 'running');
  if (!job) return 'Library scan: Idle';
  const libraryName = libraries.find((library) => library.id === job.library_id)?.name ?? String(job.library_id);
  return `Library scan: ${libraryName} • ${job.files_scanned.toLocaleString()} / ${job.files_found.toLocaleString()} files`;
}

export function describeDeepAnalysis(status: BoogieMixDeepAnalysisStatus | null): string {
  if (!status) return 'BoogieMix deep analysis: Status unavailable';
  const summary = status.runtime?.summary ?? 'Status unavailable';
  const queue = status.queue;
  return `BoogieMix deep analysis • ${summary} • Queue: ${queue.pending} pending / ${queue.running} running / ${queue.failed} failed / ${queue.skipped} skipped / ${queue.done} done`;
}

function TranscodeIcon({ active }: { active: boolean }) {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
      <rect x="3" y="4" width="18" height="6" rx="2" />
      <rect x="3" y="14" width="18" height="6" rx="2" />
      <path d="M7 7h.01M7 17h.01" />
      {active && (
        <path data-testid="transcode-conversion-mark" d="M14 11v4m0 0-2-2m2 2 2-2" strokeWidth="2.2" />
      )}
    </svg>
  );
}

function ScanIcon({ active }: { active: boolean }) {
  return (
    <svg className={active ? 'sidebar-scan-spin' : undefined} width="23" height="23" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d="M20 7v5h-5" />
      <path d="M18.2 17.2A8 8 0 1 1 20 12" />
    </svg>
  );
}

function DeepAnalysisIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
      <path d="M2 12h3l2-6 3 12 3-14 3 14 2-6h4" />
    </svg>
  );
}

function StatusIndicator({
  kind,
  description,
  color,
  active = false,
  badge = 0,
  collapsed,
  tooltip,
  onActivate,
  children,
}: {
  kind: IndicatorKind;
  description: string;
  color: string;
  active?: boolean;
  badge?: number;
  collapsed: boolean;
  /** Rich tooltip body; defaults to `description`. */
  tooltip?: React.ReactNode;
  /** Makes the indicator a button (click, Enter or Space). */
  onActivate?: () => void;
  children: React.ReactNode;
}) {
  const [tooltipVisible, setTooltipVisible] = useState(false);
  const [focused, setFocused] = useState(false);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const tipRef = useRef<HTMLSpanElement | null>(null);
  const [tipPos, setTipPos] = useState<{ left: number; top: number } | null>(null);

  // The sidebar clips its overflow, so the tooltip is portalled to <body> and placed from the indicator's
  // screen position: above it (beside it when collapsed), kept inside the window.
  useLayoutEffect(() => {
    if (!tooltipVisible) {
      setTipPos(null);
      return;
    }
    const anchor = anchorRef.current?.getBoundingClientRect();
    const tip = tipRef.current?.getBoundingClientRect();
    if (!anchor || !tip) return;
    const clamp = (value: number, max: number) => Math.max(TOOLTIP_MARGIN, Math.min(value, max - TOOLTIP_MARGIN));
    const left = collapsed
      ? anchor.right + TOOLTIP_GAP
      : anchor.left + anchor.width / 2 - tip.width / 2;
    const top = collapsed
      ? anchor.top + anchor.height / 2 - tip.height / 2
      : anchor.top - tip.height - TOOLTIP_GAP;
    setTipPos({ left: clamp(left, window.innerWidth - tip.width), top: clamp(top, window.innerHeight - tip.height) });
  }, [tooltipVisible, collapsed, tooltip, description]);

  return (
    <div
      ref={anchorRef}
      data-testid={`sidebar-status-${kind}`}
      data-active={active ? 'true' : 'false'}
      role={onActivate ? 'button' : 'img'}
      tabIndex={0}
      aria-label={description}
      onClick={onActivate}
      onKeyDown={onActivate ? (event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onActivate();
        }
      } : undefined}
      style={{
        ...styles.indicator,
        ...(active ? styles.indicatorActive : {}),
        ...(focused ? styles.indicatorFocused : {}),
        ...(onActivate ? { cursor: 'pointer' } : {}),
        color,
      }}
      onMouseEnter={() => setTooltipVisible(true)}
      onMouseLeave={() => setTooltipVisible(false)}
      onFocus={() => { setFocused(true); setTooltipVisible(true); }}
      onBlur={() => { setFocused(false); setTooltipVisible(false); }}
    >
      {children}
      {badge > 0 && <span style={styles.badge}>{badge > 99 ? '99+' : badge}</span>}
      {tooltipVisible && createPortal(
        <span
          ref={tipRef}
          role="tooltip"
          style={{
            ...styles.tooltip,
            left: tipPos?.left ?? 0,
            top: tipPos?.top ?? 0,
            visibility: tipPos ? 'visible' : 'hidden',
          }}
        >
          {tooltip ?? description}
        </span>,
        document.body,
      )}
    </div>
  );
}

export default function SidebarStatusPanel({
  currentUser,
  collapsed,
  streamDirect,
  ffmpegAvailable,
  transcodeQuality,
  activeScanJobs,
  libraries,
  deepAnalysisStatus,
  stats = null,
  onLogout,
  activity = null,
  onOpenActivity,
}: Props) {
  const activityCount = activeCount(activity?.totals);
  const scanActive = activeScanJobs.some((job) => job.status === 'running');
  const deepQueueCount = deepAnalysisStatus
    ? deepAnalysisStatus.queue.pending + deepAnalysisStatus.queue.running
    : 0;
  const deepRuntimeReady = deepAnalysisStatus?.enabled && deepAnalysisStatus.runtime?.enabled;
  const deepColor = deepAnalysisStatus?.queue.failed
    ? 'var(--danger)'
    : deepQueueCount > 0
      ? 'var(--accent)'
      : deepRuntimeReady
        ? 'var(--success)'
        : deepAnalysisStatus
          ? 'var(--warning)'
          : 'var(--text-muted)';
  const transcodeColor = streamDirect
    ? 'var(--text-muted)'
    : ffmpegAvailable === true
      ? 'var(--success)'
      : ffmpegAvailable === false
        ? 'var(--warning)'
        : 'var(--text-muted)';
  const transcodingEnabled = !streamDirect;

  return (
    <div
      data-testid="sidebar-status-panel"
      style={{
        ...styles.panel,
        ...(collapsed ? styles.panelCollapsed : {}),
      }}
    >
      {!collapsed && <LibraryStatsLine stats={stats} />}
      <div
        data-testid="sidebar-status-row"
        style={{
          ...styles.statusRow,
          // One column per indicator so the admin-only Activity indicator stays on the same row.
          gridTemplateColumns: `repeat(${activity ? 4 : 3}, minmax(0, 1fr))`,
          ...(collapsed ? styles.statusRowCollapsed : {}),
        }}
      >
        <StatusIndicator
          kind="transcoding"
          description={describeTranscoding(streamDirect, ffmpegAvailable, transcodeQuality)}
          color={transcodeColor}
          active={transcodingEnabled}
          collapsed={collapsed}
        >
          <TranscodeIcon active={transcodingEnabled} />
        </StatusIndicator>
        <StatusIndicator
          kind="scan"
          description={describeScan(activeScanJobs, libraries)}
          color={scanActive ? 'var(--accent)' : 'var(--text-muted)'}
          active={scanActive}
          collapsed={collapsed}
        >
          <ScanIcon active={scanActive} />
        </StatusIndicator>
        <StatusIndicator
          kind="deep-analysis"
          description={describeDeepAnalysis(deepAnalysisStatus)}
          color={deepColor}
          active={deepQueueCount > 0}
          badge={deepQueueCount}
          collapsed={collapsed}
        >
          <DeepAnalysisIcon />
        </StatusIndicator>
        {activity && (
          <StatusIndicator
            kind="activity"
            description={describeActivity(activity)}
            color={activityCount > 0 ? 'var(--accent)' : 'var(--text-muted)'}
            active={activityCount > 0}
            badge={activityCount}
            collapsed={collapsed}
            tooltip={<ActivitySummaryPopover activity={activity} />}
            onActivate={onOpenActivity}
          >
            <ActivityIcon size={22} />
          </StatusIndicator>
        )}
        {collapsed && (
          <StatusIndicator
            kind="library"
            description={formatLibraryStatsTitle(stats)}
            color="var(--text-muted)"
            collapsed={collapsed}
            tooltip={<LibraryStatsPopover stats={stats} />}
          >
            <LibraryIcon />
          </StatusIndicator>
        )}
      </div>

      <div style={{ ...styles.identityRow, ...(collapsed ? styles.identityRowCollapsed : {}) }}>
        <div style={styles.avatarCell}>
          <div style={styles.avatar} aria-hidden="true">{currentUser.username.slice(0, 2).toUpperCase()}</div>
        </div>
        {!collapsed && (
          <div style={styles.identityText}>
            <div style={styles.username}>{currentUser.username}</div>
            <div style={styles.role}>{currentUser.role}</div>
          </div>
        )}
        <div style={styles.logoutCell}>
          <button
            type="button"
            onClick={onLogout}
            title={`Log out ${currentUser.username}`}
            aria-label={`Log out ${currentUser.username}`}
            style={styles.logoutButton}
          >
            ⏻
          </button>
        </div>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  panel: {
    flexShrink: 0,
    margin: '0 12px 12px',
    border: '1px solid color-mix(in srgb, var(--border) 78%, transparent)',
    borderRadius: 10,
    backgroundColor: 'color-mix(in srgb, var(--surface) 72%, var(--bg))',
  },
  panelCollapsed: {
    margin: '0 8px 8px',
    borderRadius: 10,
  },
  // Two content-sized columns, centred as a block so the left and right
  // margins match regardless of digit count (a wrapping flex row went ragged
  // on large libraries).
  statsLine: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, max-content)',
    justifyContent: 'center',
    columnGap: 18,
    rowGap: 5,
    padding: '10px 12px 9px',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
    color: 'var(--text-muted)',
    fontSize: 12,
    fontWeight: 500,
    fontVariantNumeric: 'tabular-nums',
    lineHeight: 1.2,
  },
  statsLineItem: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 5,
    whiteSpace: 'nowrap',
  },
  statIcon: {
    color: 'var(--accent)',
    opacity: 0.85,
    flexShrink: 0,
  },
  statsPopover: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    minWidth: 180,
  },
  statsPopoverTitle: {
    marginBottom: 2,
    fontWeight: 700,
  },
  statsPopoverRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    fontWeight: 500,
    color: 'var(--text-muted)',
  },
  statsPopoverName: {
    flex: 1,
  },
  statsPopoverValue: {
    color: 'var(--text)',
    fontWeight: 600,
    fontVariantNumeric: 'tabular-nums',
  },
  statusRow: {
    minHeight: 58,
    padding: '8px 10px',
    display: 'grid',
    alignItems: 'center',
    justifyItems: 'center',
    borderBottom: '1px solid color-mix(in srgb, var(--border) 68%, transparent)',
  },
  statusRowCollapsed: {
    padding: '8px 5px',
    gridTemplateColumns: '1fr',
    gap: 4,
  },
  indicator: {
    position: 'relative',
    width: 38,
    height: 38,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    // Longhand (not the `border` shorthand) so `indicatorActive` can toggle
    // borderColor on/off without React warning about conflicting properties.
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: 'transparent',
    borderRadius: 8,
    outline: 'none',
  },
  indicatorActive: {
    borderColor: 'color-mix(in srgb, currentColor 72%, transparent)',
    backgroundColor: 'color-mix(in srgb, currentColor 10%, transparent)',
  },
  indicatorFocused: {
    boxShadow: 'var(--focus-ring)',
  },
  badge: {
    position: 'absolute',
    top: 1,
    right: 0,
    minWidth: 15,
    height: 15,
    padding: '0 4px',
    borderRadius: 8,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: 'var(--accent)',
    color: 'var(--on-accent)',
    border: '1px solid var(--bg)',
    fontSize: 11,
    fontWeight: 800,
    lineHeight: 1,
  },
  tooltip: {
    position: 'fixed',
    zIndex: 1000,
    fontFamily: 'var(--font), system-ui, sans-serif',
    width: 'max-content',
    maxWidth: 280,
    padding: '7px 9px',
    borderRadius: 6,
    border: '1px solid var(--border-strong)',
    backgroundColor: 'var(--surface-raised)',
    color: 'var(--text)',
    boxShadow: 'var(--shadow-subtle)',
    fontSize: 13,
    fontWeight: 600,
    lineHeight: 1.4,
    whiteSpace: 'normal',
    pointerEvents: 'none',
  },
  identityRow: {
    minHeight: 58,
    padding: '8px 10px',
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) auto minmax(0, 1fr)',
    alignItems: 'center',
    gap: 9,
  },
  identityRowCollapsed: {
    minHeight: 0,
    padding: '8px 5px',
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
  },
  avatarCell: {
    display: 'flex',
    justifyContent: 'center',
  },
  avatar: {
    width: 32,
    height: 32,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: '50%',
    backgroundColor: 'color-mix(in srgb, var(--accent) 28%, var(--surface))',
    color: 'var(--text)',
    border: '1px solid color-mix(in srgb, var(--accent) 38%, var(--border))',
    fontSize: 13,
    fontWeight: 700,
  },
  identityText: {
    minWidth: 0,
    textAlign: 'left',
  },
  logoutCell: {
    display: 'flex',
    justifyContent: 'center',
  },
  username: {
    color: 'var(--text)',
    fontSize: 14,
    fontWeight: 650,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  role: {
    marginTop: 1,
    color: 'var(--text-muted)',
    fontSize: 12,
  },
  logoutButton: {
    width: 38,
    height: 38,
    flexShrink: 0,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 0,
    backgroundColor: 'transparent',
    border: 'none',
    borderRadius: 8,
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontSize: 19,
    lineHeight: 1,
  },
};
