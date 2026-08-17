/**
 * Defines Index behavior for BoogieBox.
 */

import type { EntityId } from '../entityId';

/** Client Entity Id is part of this module's public API. */
export type ClientEntityId = EntityId;

/** Library Folder is part of this module's public API. */
export interface LibraryFolder {
  id: ClientEntityId;
  library_id: ClientEntityId;
  path: string;
  position: number;
  added_at?: string | null;
}

/** Library is part of this module's public API. */
export interface Library {
  id: ClientEntityId;
  path: string | null;
  primary_path?: string | null;
  name: string;
  library_type?: 'music';
  scanner_profile?: string;
  metadata_mode?: string;
  added_at: string;
  last_scan: string | null;
  track_count: number;
  folder_count?: number;
  folders?: LibraryFolder[];
}

/** Track is part of this module's public API. */
export interface Track {
  id: ClientEntityId;
  file_path?: string;
  file_name: string;
  file_size: number | null;
  format: string | null;
  duration: number | null;
  bitrate: number | null;
  sample_rate: number | null;
  channels: number | null;
  title: string | null;
  artist: string | null;
  album: string | null;
  library_name: string | null;
  track_number: number | null;
  disc_number: number | null;
  year: number | null;
  genre: string | null;
  composer: string | null;
  comment: string | null;
  bpm: number | null;
  bpm_detected?: number | null;
  bpm_source?: string | null;
  bpm_confidence?: number | null;
  album_id?: ClientEntityId | null;
  scanned_at: string;
  last_played_at?: string | null;
  play_count?: number | null;
  rating?: number | null;
  has_deep_analysis?: boolean;
}

/** Artist is part of this module's public API. */
export interface Artist {
  id: ClientEntityId;
  name: string;
  track_count: number;
  album_count: number;
  rating?: number | null;
  play_count?: number | null;
  metadata_locked?: number;
  description?: string | null;
  styles?: string[];
}

/** A collection-owned artist resolved from provider similarity metadata. */
export interface SimilarArtist extends Artist {
  score: number;
  providers: Array<'lastfm' | 'deezer'>;
}

/** Similar artists response for one local source artist. */
export interface SimilarArtistsResponse {
  sourceArtistId: ClientEntityId;
  artists: SimilarArtist[];
}

/** Artist Browse Page is part of this module's public API. */
export interface ArtistBrowsePage {
  items: Artist[];
  total: number;
  limit: number;
  offset: number;
  has_more: boolean;
}

/** Album is part of this module's public API. */
export interface Album {
  id: ClientEntityId;
  title: string;
  artist: string | null;
  album_artist: string | null;
  year: number | null;
  genre: string | null;
  releaseType?: 'album' | 'single' | 'compilation';
  track_count: number;
  total_duration?: number | null;
  metadata_locked?: number;
  description?: string | null;
  label?: string | null;
  rating?: number | null;
}

/** Latest Album is part of this module's public API. */
export interface LatestAlbum extends Album {
  added_at: string | null;
  latest_scanned_at: string | null;
}

/** Genre is part of this module's public API. */
export interface Genre {
  genre: string;
  track_count: number;
}

/** Home Genre Summary is part of this module's public API. */
export interface HomeGenreSummary {
  label: string;
  canonical_key: string;
  track_count: number;
  artist_count: number;
  album_count: number;
  raw_labels: string[];
}

/** Stats is part of this module's public API. */
export interface Stats {
  total_tracks: number;
  total_artists: number;
  total_albums: number;
  total_libraries: number;
  total_hours: number | null;
  total_gb: number | null;
}

/** Scan Job is part of this module's public API. */
export interface ScanJob {
  id: ClientEntityId;
  library_id: ClientEntityId;
  started_at: string;
  finished_at: string | null;
  status: 'pending' | 'running' | 'done' | 'failed' | 'error';
  files_found: number;
  files_scanned: number;
  errors: number;
  currentFile?: string;
  queue_position?: number | null;
  running_job?: {
    id: ClientEntityId;
    library_id: ClientEntityId;
    started_at: string;
    library_name: string;
  } | null;
}

/** Search Top Result is part of this module's public API. */
export interface SearchTopResult {
  type: 'track' | 'artist' | 'album';
  id: ClientEntityId;
  title: string;
  subtitle?: string | null;
  image_url?: string | null;
  library_id?: ClientEntityId | null;
  library_type?: 'music' | null;
  year?: number | null;
  score: number;
  match_reason?: 'exact' | 'prefix' | 'fts' | 'metadata' | 'library';
}

/** Search Group Summary is part of this module's public API. */
export interface SearchGroupSummary {
  total: number;
  has_more: boolean;
}

/** Search Result is part of this module's public API. */
export interface SearchResult {
  tracks: Track[];
  total: number;
  page: number;
  limit: number;
  hasMore?: boolean;
  artists: Artist[];
  albums: Album[];
  top_results?: SearchTopResult[];
  groups?: {
    tracks: SearchGroupSummary;
    artists: SearchGroupSummary;
    albums: SearchGroupSummary;
  };
}

/** Home Top Rated is part of this module's public API. */
export interface HomeTopRated {
  artists: Artist[];
  albums: Album[];
  tracks: Track[];
}

/** Sort Field is part of this module's public API. */
export type SortField = 'title' | 'artist' | 'album' | 'year' | 'duration' | 'bitrate' | 'genre' | 'rating' | 'relevance';
/** Sort Order is part of this module's public API. */
export type SortOrder = 'asc' | 'desc';

/** App Settings is part of this module's public API. */
export interface AppSettings {
  // Theme colors
  colorBg: string;
  colorSurface: string;
  colorBorder: string;
  colorAccent: string;
  colorText: string;
  colorTextMuted: string;
  bgTexture: string;
  fontFamily: string;
  // API keys
  lastfmKey: string;
  // DLNA
  dlnaEnabled: string;
  dlnaFriendlyName: string;
  dlnaPort: string;
  // Crossfade
  crossfadeMode: string;
  crossfadeDuration: string;
  // Shared playback settings (admin-controlled, applies to all users)
  vinylMode: string;
  transcodeQuality: string;
  replayGainEnabled: string;
  lastfmConfigured: string;
  // Waveforms
  waveformGenerateOnMissing: string;
  waveformBackgroundEnabled: string;
  waveformBackgroundFrequencyHours: string;
  waveformBackgroundBatchSize: string;
  bpmBackgroundEnabled: string;
  bpmBackgroundFrequencyHours: string;
  scanDebugLoggingEnabled: string;
  deepmixDebugLoggingEnabled: string;
  boogiemixOutputFolder: string;
  boogiemixDeepAnalysisBackgroundMode: string;
  boogiemixDeepAnalysisPauseBackground: string;
  boogiemixDeepAnalysisMaxDurationMins: string;
  boogiemixDeepAnalysisModel: string;
}

/** Scan Schedule is part of this module's public API. */
export interface ScanSchedule {
  id: ClientEntityId;
  library_id: ClientEntityId;
  library_name: string;
  library_path: string;
  enabled: number;
  frequency_hours: number;
  last_run: string | null;
  next_run: string | null;
  created_at: string;
}

/** Admin Queue Entry is part of this module's public API. */
export interface AdminQueueEntry {
  id: ClientEntityId;
  status: string;
  library_id: ClientEntityId | null;
  library_name: string | null;
  job_type: string | null;
  files_scanned: number | null;
  files_found: number | null;
  errors: number | null;
  started_at: string | null;
  finished_at: string | null;
  current_step: string | null;
  playlist_name: string | null;
  track_title: string | null;
  error_message: string | null;
}

/** Admin Post Scan Job Type is part of this module's public API. */
export type AdminPostScanJobType =
  | 'refresh_library_mappings'
  | 'cache_artist_images'
  | 'cache_album_images'
  | 'warm_lastfm_info'
  | 'warm_track_lyrics'
  | 'sync_artist_styles';

/** Admin Queue Snapshot is part of this module's public API. */
export interface AdminQueueSnapshot {
  fetched_at: string;
  queues: {
    scan: AdminQueueEntry[];
    postScan: AdminQueueEntry[];
    mix: AdminQueueEntry[];
    deepAnalysis: AdminQueueEntry[];
  };
}

/** DEFAULT SETTINGS is part of this module's public API. */
export const DEFAULT_SETTINGS: AppSettings = {
  colorBg:        '#161312',
  colorSurface:   '#211c1a',
  colorBorder:    '#3a2f2b',
  colorAccent:    '#d08b52',
  colorText:      '#f2ece5',
  colorTextMuted: '#b6a79b',
  bgTexture:      'none',
  fontFamily:     'Aptos, "Segoe UI Variable", "Segoe UI", Inter, system-ui, sans-serif',
  lastfmKey:      '',
  dlnaEnabled:      'false',
  dlnaFriendlyName: 'BoogieBox',
  dlnaPort:         '8200',
  crossfadeMode:     'off',
  crossfadeDuration: '2',
  vinylMode: 'standard',
  transcodeQuality: 'low',
  replayGainEnabled: 'false',
  lastfmConfigured: 'false',
  waveformGenerateOnMissing: 'true',
  waveformBackgroundEnabled: 'false',
  waveformBackgroundFrequencyHours: '24',
  waveformBackgroundBatchSize: '100',
  bpmBackgroundEnabled: 'false',
  bpmBackgroundFrequencyHours: '24',
  scanDebugLoggingEnabled: 'false',
  deepmixDebugLoggingEnabled: 'false',
  boogiemixOutputFolder: '',
  boogiemixDeepAnalysisBackgroundMode: 'off',
  boogiemixDeepAnalysisPauseBackground: 'false',
  boogiemixDeepAnalysisMaxDurationMins: '15',
  boogiemixDeepAnalysisModel: 'mdx_extra_q',
};

/** FONT OPTIONS is part of this module's public API. */
export const FONT_OPTIONS = [
  { label: 'Aptos / Segoe',    value: 'Aptos, "Segoe UI Variable", "Segoe UI", Inter, system-ui, sans-serif' },
  { label: 'Manrope',          value: 'Manrope, "Segoe UI Variable", "Segoe UI", Inter, system-ui, sans-serif' },
  { label: 'Plus Jakarta Sans', value: '"Plus Jakarta Sans", "Segoe UI Variable", "Segoe UI", Inter, system-ui, sans-serif' },
  { label: 'Space Grotesk',    value: '"Space Grotesk", "Segoe UI Variable", "Segoe UI", Inter, system-ui, sans-serif' },
  { label: 'IBM Plex Mono',    value: 'IBM Plex Mono' },
  { label: 'Fira Code',        value: 'Fira Code' },
  { label: 'JetBrains Mono',   value: 'JetBrains Mono' },
  { label: 'Source Code Pro',  value: 'Source Code Pro' },
  { label: 'Roboto Mono',      value: 'Roboto Mono' },
  { label: 'Inter',            value: 'Inter' },
  { label: 'System UI',        value: 'system-ui, sans-serif' },
];

/** Playlist is part of this module's public API. */
export interface Playlist {
  id: EntityId;
  name: string;
  description: string | null;
  created_at: string | null;
  updated_at: string | null;
  track_count: number;
  total_duration: number | null;
  art_album_ids?: ClientEntityId[];
  remember_progress?: number;
}

/** Playlist Track is part of this module's public API. */
export interface PlaylistTrack extends Track {
  position: number;
  playlist_track_id: ClientEntityId | null;
  progress_seconds?: number;
}

/** Boogie Mix Transition is part of this module's public API. */
export interface BoogieMixTransition {
  step_index: number;
  from_track_id: ClientEntityId;
  to_track_id: ClientEntityId;
  crossfade_sec: number;
  from_outro_start_sec: number | null;
  to_intro_start_sec: number | null;
  phrase_aware: number;
  reason: string | null;
}

/** Boogie Mix Log is part of this module's public API. */
export interface BoogieMixLog {
  level: 'info' | 'warn' | 'error';
  message: string;
  created_at: string;
}

/** Boogie Mix Job is part of this module's public API. */
export interface BoogieMixJob {
  id: ClientEntityId;
  playlist_id: EntityId;
  user_id: EntityId;
  status: 'pending' | 'analyzing' | 'planning' | 'rendering' | 'done' | 'failed' | 'canceled';
  progress_percent: number;
  current_step: string;
  last_message: string | null;
  cancel_requested: number;
  default_crossfade_sec: number;
  output_id: ClientEntityId | null;
  mix_style?: 'chill_blend' | 'club_blend' | 'long_build' | 'safe_mix';
  mix_quality?: 'standard' | 'high_quality';
  used_deep_analysis?: boolean | number;
  deep_analysis_status?: string | null;
  deep_analysis_ready_count?: number;
  deep_analysis_total_count?: number;
  deep_analysis_missing_reason?: string | null;
  started_at: string | null;
  finished_at: string | null;
  created_at: string;
  updated_at: string;
  transitions: BoogieMixTransition[];
  logs: BoogieMixLog[];
  mix_strategy?: string | null;
  planner_provider?: string | null;
  plan_summary?: {
    style: string | null;
    energyCurvePhases: string[];
    orderedTrackIds: ClientEntityId[];
    anthemTrackId: ClientEntityId | null;
  } | null;
}

/** Boogie Mix Output is part of this module's public API. */
export interface BoogieMixOutput {
  id: ClientEntityId;
  job_id: ClientEntityId;
  playlist_id: EntityId;
  file_name: string;
  duration_sec: number | null;
  file_size_bytes: number | null;
  format: string;
  created_at: string;
}

/** Boogie Mix Deep Analysis Status is part of this module's public API. */
export interface BoogieMixDeepAnalysisStatus {
  enabled: boolean;
  runtime: {
    pythonAvailable: boolean;
    ffmpegAvailable: boolean;
    demucsCallable: boolean;
    torchAvailable: boolean;
    gpuAvailable: boolean;
    enabled: boolean;
    details: string[];
    missingCapabilities: string[];
    summary: string;
    python: BoogieMixRuntimeComponentStatus;
    ffmpeg: BoogieMixRuntimeComponentStatus;
    demucs: BoogieMixRuntimeComponentStatus;
    torch: BoogieMixRuntimeComponentStatus;
    gpu: BoogieMixRuntimeComponentStatus;
  } | null;
  queue: {
    pending: number;
    running: number;
    failed: number;
    skipped: number;
    done: number;
  };
  cache: {
    analyzedTracks: number;
    estimatedBytes: number;
    oldestCreatedAt: string | null;
    newestCreatedAt: string | null;
  };
  controls?: {
    backgroundMode: 'off' | 'playlists_only' | 'favorites_and_playlists' | 'all_music';
    pauseBackground: boolean;
  };
}

/** Per-playlist deep analysis progress returned by the progress endpoint. */
export interface PlaylistDeepAnalysisProgress {
  total: number;
  pending: number;
  running: number;
  done: number;
  failed: number;
  skipped: number;
  notQueued: number;
  /** Tracks with any saved deep-analysis cache row. */
  analyzedCached?: number;
  /** Tracks with real (non-synthetic) Demucs analysis stored. */
  analyzedReal: number;
  /** Tracks with saved synthetic fallback rows. */
  analyzedFallback?: number;
}

/** Boogie Mix Runtime Component Status is part of this module's public API. */
export interface BoogieMixRuntimeComponentStatus {
  available: boolean;
  version: string | null;
  detail: string | null;
}

// ─── Crossfade ─────────────────────────────────────────────────────────────

/** Crossfade Mode is part of this module's public API. */
export type CrossfadeMode = 'off' | 'zerogap' | 'crossfade';

/** Crossfade Config is part of this module's public API. */
export interface CrossfadeConfig {
  mode: CrossfadeMode;
  duration: number;
  source: 'global' | 'override';
}

/** Crossfade Override is part of this module's public API. */
export interface CrossfadeOverride {
  id: ClientEntityId;
  entity_type: 'album' | 'playlist' | 'autodj';
  entity_id: ClientEntityId;
  mode: CrossfadeMode;
  duration: number;
}

/** Track Waveform is part of this module's public API. */
export interface TrackWaveform {
  trackId: ClientEntityId;
  sampleCount: number;
  durationSeconds: number | null;
  points: number[];
  updatedAt: string;
}

/** Track Waveform Lookup Response is part of this module's public API. */
export type TrackWaveformLookupResponse =
  | { status: 'ready'; waveform: TrackWaveform }
  | { status: 'missing' | 'generating' }
  | { status: 'error'; error: string };

/** Waveform Map Run Result is part of this module's public API. */
export interface WaveformMapRunResult {
  started: boolean;
  inProgress: boolean;
  reason: string;
  startedAt: string;
  finishedAt: string;
  batchSize: number;
  totalMissing: number;
  processed: number;
  generated: number;
  skipped: number;
  errors: number;
}

/** Waveform Mapping Status is part of this module's public API. */
export interface WaveformMappingStatus {
  enabled: boolean;
  generateOnMissing: boolean;
  frequencyHours: number;
  batchSize: number;
  lastRun: string | null;
  nextRun: string | null;
  inProgress: boolean;
  totalTracks: number;
  mappedTracks: number;
  missingTracks: number;
  activeRun: {
    reason: string;
    startedAt: string;
    totalMissing: number;
    processed: number;
    generated: number;
    skipped: number;
    errors: number;
  } | null;
}

/** Bpm Analysis Status is part of this module's public API. */
export interface BpmAnalysisStatus {
  enabled: boolean;
  backgroundEnabled: boolean;
  frequencyHours: number;
  lastRun: string | null;
  nextRun: string | null;
  spotifyFallbackEnabled: boolean;
  totalTracks: number;
  analyzedTracks: number;
  missingTracks: number;
  inProgress: boolean;
  activeRun: {
    processed: number;
    analyzed: number;
    skipped: number;
    errors: number;
  } | null;
}

/** Bpm Batch Result is part of this module's public API. */
export interface BpmBatchResult {
  processed: number;
  analyzed: number;
  skipped: number;
  errors: number;
}

/** Queue Source Type is part of this module's public API. */
export type QueueSourceType = 'album' | 'playlist' | 'autodj' | 'search' | 'single';

/** Queue Source is part of this module's public API. */
export interface QueueSource {
  type: QueueSourceType;
  id: ClientEntityId;
  rememberProgress?: boolean;
}

// ─── Last.fm ───────────────────────────────────────────────────────────────

/** Last Fm Info is part of this module's public API. */
export interface LastFmInfo {
  summary: string;
  full: string;
  listeners?: string;
  playcount?: string;
  url?: string;
  image?: string;
  tags?: string[];
}

// ─── Auth / Users ──────────────────────────────────────────────────────────

/** Auth User is part of this module's public API. */
export interface AuthUser {
  id: EntityId;
  username: string;
  role: 'admin' | 'user';
  canManageLibraries: boolean;
  canEditMetadata: boolean;
}

/** Login User is part of this module's public API. */
export interface LoginUser {
  id: EntityId;
  username: string;
}

/** Admin User is part of this module's public API. */
export interface AdminUser {
  id: EntityId;
  username: string;
  role: 'admin' | 'user';
  hasPin: boolean;
  canManageLibraries: boolean;
  canEditMetadata: boolean;
  created_at: string;
}

/** Provider Usage Row is part of this module's public API. */
export interface ProviderUsageRow {
  provider: string;
  entity_type: string;
  usage_type: string;
  count: number;
  last_used_at: string;
}

/** Provider Usage Provider Summary is part of this module's public API. */
export interface ProviderUsageProviderSummary {
  provider: string;
  total_count: number;
  last_used_at: string | null;
  usage_breakdown: Record<string, number>;
}

/** Provider Usage Snapshot is part of this module's public API. */
export interface ProviderUsageSnapshot {
  fetched_at: string;
  providers: ProviderUsageProviderSummary[];
  rows: ProviderUsageRow[];
}

/** History Entry is part of this module's public API. */
export interface HistoryEntry {
  id: ClientEntityId;
  track_id: ClientEntityId;
  played_at: string;
  title: string | null;
  artist: string | null;
  album: string | null;
}

// ─── Sonic Fingerprint ─────────────────────────────────────────────────────

/** A detected structural section within a track (intro, verse, chorus, etc.). */
export interface TrackSection {
  kind: string;
  start: number;
  end: number;
  confidence: number;
  vocalDensity: number;
  drumDensity: number;
  energy: number;
}

/** A time window of stem activity (vocals, drums, or bass). */
export interface StemWindow {
  start: number;
  end: number;
  strength: number;
  average: number;
}

/** A time window suitable for DJ transitions. */
export interface TransitionWindow {
  role: string;
  start: number;
  end: number;
  score: number;
  vocalRisk: number;
  drumContinuity: number;
  bassRisk: number;
  energy: number;
  recommendedMinCrossfade?: number;
  recommendedMaxCrossfade?: number;
  recommended?: boolean;
}

/** Refined intro/outro boundary timestamps. */
export interface IntroOutroRefined {
  introEnd: number | null;
  outroStart: number | null;
}

/** Full Sonic Fingerprint payload returned by GET /api/tracks/:id/sonic-fingerprint. */
export interface SonicFingerprint {
  trackId: string;
  bpmDetected: number | null;
  energyScoreRefined: number;
  confidence: number;
  sourceDurationSec: number | null;
  demucsModel: string;
  usedGpu: boolean;
  analysisSchemaVersion: number;
  sectionJson: TrackSection[];
  vocalWindowsJson: StemWindow[];
  drumWindowsJson: StemWindow[];
  bassWindowsJson: StemWindow[];
  transitionWindowsJson: TransitionWindow[];
  introOutroRefinedJson: IntroOutroRefined;
  phraseBoundariesJson: number[];
}

/** Metadata Search Result is part of this module's public API. */
export interface MetadataSearchResult {
  provider: 'lastfm' | 'discogs' | 'spotify' | 'deezer';
  type: 'artist' | 'album';
  title: string;
  artist?: string;
  year?: number | string;
  genre?: string;
  tags?: string[];
  image?: string;
  url?: string;
  releaseType?: 'album' | 'single' | 'compilation';
}
