/** Wording and number formatting for the Activity page and sidebar summary (copy approved 2026-10-24). */

import { formatBytes } from '../utils';
import type {
  ActiveMixJob, ActivitySource, DeliveryInfo, DownloadRow, PlayingRow, RecentRow, ServerJobs,
} from './activityTypes';

export { formatBytes };

/** `9.4 MB/s`. */
export function formatRate(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** `m:ss`, or `h:mm:ss` from an hour. */
export function formatClock(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return '0:00';
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** `just now`, `4 min ago`, `3 h ago`, then a date. */
export function formatRelative(atMs: number, nowMs: number = Date.now()): string {
  const seconds = Math.max(0, Math.round((nowMs - atMs) / 1000));
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(atMs).toLocaleDateString();
}

/** Source chip: `Web player`, `Play On · Living room`, `Subsonic · Amperfy`, `DLNA · Bedroom TV`, `Radio`. */
export function sourceLabel(source: ActivitySource, client: string | null): string {
  const named = client?.trim() || null;
  switch (source) {
    case 'web':
    case 'mixFile':
      return 'Web player';
    case 'playOn':
      return named ? `Play On · ${named}` : 'Play On';
    case 'subsonic':
      return named ? `Subsonic · ${named}` : 'Subsonic';
    case 'dlna':
      return named ? `DLNA · ${named}` : 'DLNA device';
    case 'radio':
      return 'Radio';
    default:
      return named ?? '';
  }
}

/** `Original file`, `FLAC → MP3 192 kbps`, `WAV for Living room`. */
export function deliveryLabel(delivery: DeliveryInfo | null, source: ActivitySource, client: string | null): string | null {
  if (!delivery) return null;
  if (!delivery.transcoded) return 'Original file';
  const target = delivery.targetFormat ?? '';
  if (target === 'WAV' && source === 'playOn' && client) return `WAV for ${client}`;
  const kbps = delivery.kbps ? ` ${delivery.kbps} kbps` : '';
  return delivery.sourceFormat ? `${delivery.sourceFormat} → ${target}${kbps}` : `${target}${kbps}`;
}

/** Position text: `2:14 / 4:05`, `~2:14 / 4:05` when estimated. */
export function progressLabel(row: PlayingRow, positionSec: number | null): string | null {
  if (positionSec == null) return null;
  const pos = `${row.estimated ? '~' : ''}${formatClock(positionSec)}`;
  return row.durationSec ? `${pos} / ${formatClock(row.durationSec)}` : pos;
}

/** Second line of a download row. */
export function downloadDetail(row: DownloadRow): string {
  if (row.label === 'file') {
    return row.totalBytes ? `${formatBytes(row.bytes)} / ${formatBytes(row.totalBytes)}` : formatBytes(row.bytes);
  }
  const songs = row.itemsCompleted + row.inProgress;
  return `${songs} ${songs === 1 ? 'song' : 'songs'} · ${formatBytes(row.bytes)} in the last 5 min`;
}

const MIX_STEP_LABEL: Record<string, string> = {
  pending: 'Queued', analyzing: 'Analyzing', planning: 'Planning', rendering: 'Rendering',
};

/** `Rendering transitions · 64%`. */
export function mixStepLabel(job: ActiveMixJob): string {
  const step = job.step && job.step !== 'queued' ? job.step : MIX_STEP_LABEL[job.status] ?? job.status;
  return `${step.charAt(0).toUpperCase()}${step.slice(1)} · ${job.progressPercent}%`;
}

const STYLE_LABEL: Record<string, string> = {
  chill_blend: 'Chill blend', club_blend: 'Club blend', long_build: 'Long build', safe_mix: 'Safe mix',
};

export function mixStyleLabel(style: string): string {
  return STYLE_LABEL[style] ?? style;
}

const POST_SCAN_LABEL: Record<string, string> = {
  artwork: 'artwork', art: 'artwork', waveforms: 'waveforms', waveform: 'waveforms', bpm: 'BPM',
  lastfm: 'Last.fm', lyrics: 'lyrics', artist_styles: 'artist styles',
};

/** `Queued after scan: artwork, waveforms, BPM` or null. */
export function postScanLabel(jobs: ServerJobs): string | null {
  if (!jobs.postScanQueued.length) return null;
  const names = jobs.postScanQueued.map(([kind]) => POST_SCAN_LABEL[kind] ?? kind.replace(/_/g, ' '));
  return `Queued after scan: ${[...new Set(names)].join(', ')}`;
}

/** One "Recent" line. */
export function recentText(row: RecentRow): string {
  const who = row.user ?? 'Someone';
  const title = row.title ?? 'a track';
  switch (row.kind) {
    case 'play':
      return `${who} played ${title}`;
    case 'stream':
      if (row.source === 'dlna') return `${row.client || 'DLNA device'} played ${title}`;
      return `${who} played ${title}`;
    case 'download':
      if (row.source === 'mixFile') return `${who} downloaded ${title}`;
      return `${who} downloaded ${row.count ?? 0} ${row.count === 1 ? 'song' : 'songs'}`;
    case 'mixFinished':
      return `${who}'s mix ${row.title ?? ''} finished`.replace(/\s+/g, ' ').trim();
    case 'mixFailed':
      return `${who}'s mix ${row.title ?? ''} failed`.replace(/\s+/g, ' ').trim();
    case 'scanFinished':
      return `Scan of ${row.title ?? 'a library'} finished`;
    case 'playOnStopped':
      return `Play On · ${row.client ?? 'speaker'} stopped`;
    default:
      return title;
  }
}

/** Secondary line of a "Recent" entry: app or speaker, then time. */
export function recentMeta(row: RecentRow, nowMs: number = Date.now()): string {
  const when = formatRelative(row.atMs, nowMs);
  const showClient = row.client && (row.kind === 'play' || row.kind === 'download') && row.source !== 'web';
  return showClient ? `${row.client} · ${when}` : when;
}
