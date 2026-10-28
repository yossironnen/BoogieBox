/**
 * Shared bits of the backup UI: icons, labels and date/size formatting (desktop Settings, mobile admin, sidebar,
 * restore screens).
 */

import React from 'react';
import type { BackupReason } from '../types';

type IconProps = { size?: number; color?: string; style?: React.CSSProperties };

function Svg({ size = 16, color = 'currentColor', style, children, strokeWidth = 2 }: IconProps & { children: React.ReactNode; strokeWidth?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      {children}
    </svg>
  );
}

export const BackupIcons = {
  LastBackup: (p: IconProps) => <Svg {...p}><path d="M21 12a9 9 0 1 1-3-6.7L21 8" /><path d="M21 3v5h-5" /><path d="m9 12 2 2 4-4" /></Svg>,
  Calendar: (p: IconProps) => <Svg {...p}><rect x="3" y="4" width="18" height="18" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" /></Svg>,
  Archive: (p: IconProps) => <Svg {...p}><path d="M3 7h18v13a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z" /><path d="M1 3h22v4H1z" /><path d="M10 12h4" /></Svg>,
  Download: (p: IconProps) => <Svg {...p}><path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M5 21h14" /></Svg>,
  FolderUp: (p: IconProps) => <Svg {...p}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><path d="M12 10v6" /><path d="m9 13 3-3 3 3" /></Svg>,
  FolderAlert: (p: IconProps) => <Svg {...p}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><path d="M12 11v3" /><path d="M12 17h.01" /></Svg>,
  Restore: (p: IconProps) => <Svg {...p}><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" /></Svg>,
  Trash: (p: IconProps) => <Svg {...p}><path d="M3 6h18" /><path d="M8 6V4h8v2" /><path d="M19 6l-1 14H6L5 6" /></Svg>,
  Lock: (p: IconProps) => <Svg {...p}><rect x="4" y="11" width="16" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></Svg>,
  Warning: (p: IconProps) => <Svg {...p}><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /><path d="M12 9v4" /><path d="M12 17h.01" /></Svg>,
  Clock: (p: IconProps) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></Svg>,
  Tracks: (p: IconProps) => <Svg {...p}><path d="M9 18V5l10-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="16" cy="16" r="3" /></Svg>,
  Playlists: (p: IconProps) => <Svg {...p}><path d="M3 6h13M3 12h13M3 18h9" /><path d="M19 15v6M16 18h6" /></Svg>,
  Users: (p: IconProps) => <Svg {...p}><circle cx="9" cy="8" r="4" /><path d="M2 21a7 7 0 0 1 14 0" /><path d="M17 11a3 3 0 1 0 0-6M22 21a6 6 0 0 0-4-5.6" /></Svg>,
  Artwork: (p: IconProps) => <Svg {...p}><rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="9" cy="9" r="2" /><path d="m21 15-5-5L5 21" /></Svg>,
  Key: (p: IconProps) => <Svg {...p}><circle cx="8" cy="15" r="4" /><path d="m10.8 12.2 9.2-9.2M17 6l3 3M14 9l2 2" /></Svg>,
  Tag: (p: IconProps) => <Svg {...p}><path d="M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8z" /><circle cx="7.5" cy="7.5" r="1.5" /></Svg>,
  CheckCircle: (p: IconProps) => <Svg {...p} strokeWidth={2.4}><circle cx="12" cy="12" r="9" /><path d="m8 12 3 3 5-6" /></Svg>,
  Check: (p: IconProps) => <Svg {...p} strokeWidth={2.4}><path d="m5 12 4 4 10-10" /></Svg>,
  Off: (p: IconProps) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="m5.6 5.6 12.8 12.8" /></Svg>,
  Sun: (p: IconProps) => <Svg {...p}><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2" /></Svg>,
  Spinner: (p: IconProps) => <Svg {...p}><path d="M21 12a9 9 0 1 1-6.2-8.6" /></Svg>,
  Desktop: (p: IconProps) => <Svg {...p}><rect x="2" y="4" width="20" height="13" rx="2" /><path d="M8 21h8M12 17v4" /></Svg>,
  Plus: (p: IconProps) => <Svg {...p}><path d="M12 5v14M5 12h14" /></Svg>,
};

export const BACKUP_REASON_LABELS: Record<BackupReason, string> = {
  scheduled: 'Scheduled',
  manual: 'Manual',
  'pre-migration': 'Before upgrade',
  'pre-restore': 'Before restore',
};

const DATE_OPTS: Intl.DateTimeFormatOptions = {
  weekday: 'short', day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false,
};

/** "Mon 28 Oct 2026, 03:30" (local time). */
export function formatBackupDate(iso: string, opts: { weekday?: boolean; year?: boolean } = {}): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const options: Intl.DateTimeFormatOptions = { ...DATE_OPTS };
  if (opts.weekday === false) delete options.weekday;
  if (opts.year === false) delete options.year;
  return date.toLocaleString('en-GB', options).replace(/^(\w{3}),? /, '$1 ');
}

/** "Today, 03:30" for today, otherwise the full date. */
export function formatRecentBackupDate(iso: string, now = new Date()): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  if (date.toDateString() === now.toDateString()) {
    return `Today, ${date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', hour12: false })}`;
  }
  return formatBackupDate(iso);
}

/** The server's local `YYYY-MM-DDTHH:MM` as "Sun 3 Nov, 03:30". */
export function formatNextRun(local: string): string {
  const [datePart, time] = local.split('T');
  const [y, m, d] = datePart.split('-').map(Number);
  const date = new Date(y, (m || 1) - 1, d || 1);
  if (Number.isNaN(date.getTime()) || !time) return local;
  const day = date.toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short' }).replace(/^(\w{3}),? /, '$1 ');
  return `${day}, ${time}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

/** UTC `pre-restore-YYYYMMDD-HHMMSS`, the folder a restore saves current data to. */
export function preRestoreFolderName(now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `pre-restore-${now.getUTCFullYear()}${p(now.getUTCMonth() + 1)}${p(now.getUTCDate())}-${p(now.getUTCHours())}${p(now.getUTCMinutes())}${p(now.getUTCSeconds())}`;
}

export const SCHEDULE_DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
