/**
 * Unified desktop-sidebar identity and server activity panel.
 */

import React, { useState } from 'react';
import type { AuthUser, BoogieMixDeepAnalysisStatus, Library, ScanJob } from '../types';

interface Props {
  currentUser: AuthUser;
  collapsed: boolean;
  streamDirect: boolean;
  ffmpegAvailable: boolean | null;
  transcodeQuality: string;
  activeScanJobs: ScanJob[];
  libraries: Library[];
  deepAnalysisStatus: BoogieMixDeepAnalysisStatus | null;
  onLogout: () => void;
}

type IndicatorKind = 'transcoding' | 'scan' | 'deep-analysis';

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
  children,
}: {
  kind: IndicatorKind;
  description: string;
  color: string;
  active?: boolean;
  badge?: number;
  collapsed: boolean;
  children: React.ReactNode;
}) {
  const [tooltipVisible, setTooltipVisible] = useState(false);
  const [focused, setFocused] = useState(false);
  return (
    <div
      data-testid={`sidebar-status-${kind}`}
      data-active={active ? 'true' : 'false'}
      role="img"
      tabIndex={0}
      aria-label={description}
      style={{
        ...styles.indicator,
        ...(active ? styles.indicatorActive : {}),
        ...(focused ? styles.indicatorFocused : {}),
        color,
      }}
      onMouseEnter={() => setTooltipVisible(true)}
      onMouseLeave={() => setTooltipVisible(false)}
      onFocus={() => { setFocused(true); setTooltipVisible(true); }}
      onBlur={() => { setFocused(false); setTooltipVisible(false); }}
    >
      {children}
      {badge > 0 && <span style={styles.badge}>{badge > 99 ? '99+' : badge}</span>}
      {tooltipVisible && (
        <span
          role="tooltip"
          style={{
            ...styles.tooltip,
            ...(kind === 'transcoding' && !collapsed ? styles.tooltipLeftAligned : {}),
            ...(collapsed ? styles.tooltipCollapsed : {}),
          }}
        >
          {description}
        </span>
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
  onLogout,
}: Props) {
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
      <div style={{ ...styles.statusRow, ...(collapsed ? styles.statusRowCollapsed : {}) }}>
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
  statusRow: {
    minHeight: 58,
    padding: '8px 10px',
    display: 'grid',
    gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
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
    position: 'absolute',
    left: '50%',
    bottom: 'calc(100% + 8px)',
    transform: 'translateX(-50%)',
    zIndex: 40,
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
  tooltipCollapsed: {
    left: 'calc(100% + 8px)',
    bottom: '50%',
    transform: 'translateY(50%)',
  },
  tooltipLeftAligned: {
    left: 0,
    transform: 'none',
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
