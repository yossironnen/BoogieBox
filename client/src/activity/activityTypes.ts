/** Admin Activity page data (`GET /api/activity`, SSE `/api/activity/events`). See wip/activity-dashboard-plan.md. */

export type ActivitySource = 'web' | 'subsonic' | 'playOn' | 'dlna' | 'radio' | 'mixFile';

export interface DeliveryInfo {
  transcoded: boolean;
  /** Source format, e.g. `FLAC`. */
  sourceFormat: string | null;
  /** Target format when transcoding, e.g. `MP3`. */
  targetFormat: string | null;
  kbps: number | null;
}

export interface ActivityTotals {
  playing: number;
  downloading: number;
  mixJobs: number;
  transcodes: number;
  subsonicTranscodeLimit: number | null;
  activeUsers: number;
  serverJobs: number;
}

export interface PlayingRow {
  id: string;
  source: ActivitySource;
  user: string | null;
  /** App name, speaker name or DLNA device name. */
  client: string | null;
  itemId: string;
  title: string | null;
  artist: string | null;
  albumId: string | null;
  positionSec: number | null;
  durationSec: number | null;
  /** Worked out from the start time, not reported by the player. */
  estimated: boolean;
  sampledAtMs: number;
  status: 'playing' | 'paused' | 'loading' | 'streaming';
  delivery: DeliveryInfo | null;
}

export interface DownloadRow {
  id: string;
  source: ActivitySource;
  user: string | null;
  client: string | null;
  label: 'offlineSync' | 'file';
  itemId: string | null;
  title: string | null;
  itemsCompleted: number;
  bytes: number;
  inProgress: number;
  rateBps: number;
  totalBytes: number | null;
  startedAtMs: number;
}

export interface ActiveMixJob {
  id: string;
  user: string | null;
  playlistName: string | null;
  style: string;
  trackCount: number;
  status: 'pending' | 'analyzing' | 'planning' | 'rendering';
  progressPercent: number;
  step: string;
  message: string | null;
  startedAt: string | null;
  canCancel: boolean;
}

export interface ScanRow {
  id: string;
  library: string | null;
  status: string;
  filesScanned: number | null;
  filesFound: number | null;
  startedAt: string | null;
}

export interface ServerJobs {
  scans: ScanRow[];
  /** `[jobType, count]` pairs. */
  postScanQueued: Array<[string, number]>;
  deepAnalysisQueued: number;
  deepAnalysisRunning: number;
}

export type RecentKind =
  | 'play' | 'download' | 'stream' | 'playOnStopped' | 'mixFinished' | 'mixFailed' | 'scanFinished';

export interface RecentRow {
  atMs: number;
  kind: RecentKind;
  user: string | null;
  source: string | null;
  client: string | null;
  itemId: string | null;
  title: string | null;
  count: number | null;
  bytes: number | null;
}

export interface ActivitySnapshot {
  generatedAtMs: number;
  totals: ActivityTotals;
  playing: PlayingRow[];
  downloads: DownloadRow[];
  mixJobs: ActiveMixJob[];
  serverJobs: ServerJobs;
  recent: RecentRow[];
}

/** Body of `POST /api/activity/heartbeat`. */
export interface PlayerHeartbeat {
  playerId: string;
  trackId: string | null;
  positionSec: number;
  durationSec: number | null;
  paused: boolean;
  stopped?: boolean;
}

/** Number shown on the sidebar indicator and nav badge. */
export function activeCount(totals: ActivityTotals | null | undefined): number {
  if (!totals) return 0;
  return totals.playing + totals.downloading + totals.mixJobs;
}
