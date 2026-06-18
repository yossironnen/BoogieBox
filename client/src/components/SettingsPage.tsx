/**
 * Defines the Settings Page React component and related UI helpers.
 */

import React, { useState, useEffect, useCallback } from 'react';
import { api, getStreamDirect, setStreamDirect } from '../api';
import type { AppSettings, ScanSchedule, WaveformMappingStatus, BpmAnalysisStatus, BoogieMixDeepAnalysisStatus, AuthUser, AdminQueueEntry, AdminQueueSnapshot, ClientEntityId, Library, AdminPostScanJobType, ProviderUsageSnapshot, ProviderUsageProviderSummary } from '../types';
import { DEFAULT_SETTINGS, FONT_OPTIONS } from '../types';
import { parseServerDate } from '../utils';
import LibrarySettingsTab from './LibrarySettingsTab';
import UserManagement from './UserManagement';
import FolderPickerModal from './FolderPickerModal';

// ─── Helpers ─────────────────────────────────────────────────────────────────

function fmtNextRun(iso: string | null): string {
  if (!iso) return 'Not scheduled';
  const d = parseServerDate(iso);
  if (!d) return 'Not scheduled';
  const now = new Date();
  const diff = d.getTime() - now.getTime();
  if (diff < 0) return 'Overdue';
  const h = Math.floor(diff / 3600000);
  const m = Math.floor((diff % 3600000) / 60000);
  if (h > 23) return `in ${Math.floor(h / 24)}d ${h % 24}h`;
  if (h > 0)  return `in ${h}h ${m}m`;
  return `in ${m}m`;
}

function fmtLastRun(iso: string | null): string {
  if (!iso) return 'Never';
  return (parseServerDate(iso) ?? new Date(iso)).toLocaleString();
}

function formatBytes(bytes: number | null | undefined): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

function fmtQueueTime(iso: string | null): string {
  if (!iso) return 'n/a';
  return (parseServerDate(iso) ?? new Date(iso)).toLocaleString();
}

function formatProviderLabel(provider: string): string {
  switch (provider) {
    case 'lastfm':
      return 'Last.fm';
    default:
      return provider.charAt(0).toUpperCase() + provider.slice(1);
  }
}

function formatQueueLine(queueName: string, entry: AdminQueueEntry): string {
  const bits = [`${queueName} #${entry.id}`, `status=${entry.status}`];
  if (entry.library_name) bits.push(`library=${entry.library_name}`);
  if (entry.job_type) bits.push(`type=${entry.job_type}`);
  if (entry.playlist_name) bits.push(`playlist=${entry.playlist_name}`);
  if (entry.track_title) bits.push(`track=${entry.track_title}`);
  if (entry.current_step) bits.push(`step=${entry.current_step}`);
  if (entry.files_found != null || entry.files_scanned != null) bits.push(`files=${entry.files_scanned ?? 0}/${entry.files_found ?? 0}`);
  if (entry.errors != null) bits.push(`errors=${entry.errors}`);
  if (entry.started_at) bits.push(`started=${fmtQueueTime(entry.started_at)}`);
  if (entry.error_message) bits.push(`note=${entry.error_message}`);
  return bits.join(' | ');
}

/** Format Queue Snapshot is part of this module's public API. */
export function formatQueueSnapshot(snapshot: AdminQueueSnapshot | null): string {
  if (!snapshot) return 'No queue snapshot loaded yet.';
  const sections: Array<{ label: string; key: string; items: AdminQueueEntry[] }> = [
    { label: 'Scan Queue', key: 'scan', items: snapshot.queues.scan },
    { label: 'Post-Scan Queue', key: 'post-scan', items: snapshot.queues.postScan },
    { label: 'Mix Queue', key: 'mix', items: snapshot.queues.mix },
    { label: 'Deep Analysis Queue', key: 'deep-analysis', items: snapshot.queues.deepAnalysis },
  ];
  const lines = [`Snapshot: ${fmtQueueTime(snapshot.fetched_at)}`];
  for (const section of sections) {
    lines.push('');
    lines.push(`${section.label} (${section.items.length})`);
    if (section.items.length === 0) {
      lines.push('  idle');
      continue;
    }
    for (const entry of section.items) lines.push(`  ${formatQueueLine(section.key, entry)}`);
  }
  return lines.join('\n');
}

function formatQueueStateLabel(status: string): string {
  switch (status) {
    case 'running':
      return 'Running';
    case 'pending':
      return 'Queued';
    case 'failed':
      return 'Failed';
    case 'done':
      return 'Done';
    case 'cancelled':
      return 'Cancelled';
    default:
      return status;
  }
}

const MUSIC_POST_SCAN_ACTIONS: Array<{ jobType: AdminPostScanJobType; label: string }> = [
  { jobType: 'refresh_library_mappings', label: 'Refresh mappings' },
  { jobType: 'cache_artist_images', label: 'Cache artist images' },
  { jobType: 'cache_album_images', label: 'Cache album images' },
  { jobType: 'warm_lastfm_info', label: 'Warm Last.fm' },
  { jobType: 'warm_track_lyrics', label: 'Warm lyrics' },
  { jobType: 'sync_artist_styles', label: 'Sync artist styles' },
];

function getLibraryPostScanActions(library: Library): Array<{ jobType: AdminPostScanJobType; label: string }> {
  return MUSIC_POST_SCAN_ACTIONS;
}

// ─── Color Swatch ─────────────────────────────────────────────────────────────

function ColorInput({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 14 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flex: 1 }}>
        <input
          type="color"
          value={value}
          onChange={e => onChange(e.target.value)}
          style={{
            width: 40, height: 32, border: '1px solid var(--border)', borderRadius: 6,
            cursor: 'pointer', backgroundColor: 'transparent', padding: 2,
          }}
        />
        <div>
          <div style={{ fontSize: 12, color: 'var(--text)', fontWeight: 500 }}>{label}</div>
          <div style={{ fontSize: 10, color: 'var(--text-muted)', fontFamily: 'monospace' }}>{value}</div>
        </div>
      </div>
      <input
        type="text"
        value={value}
        onChange={e => onChange(e.target.value)}
        style={{
          width: 100, background: 'var(--surface)', border: '1px solid var(--border)',
          color: 'var(--text)', borderRadius: 6, padding: '5px 8px', fontSize: 11,
          fontFamily: 'monospace', outline: 'none',
        }}
      />
    </div>
  );
}

// ─── Theme Presets ────────────────────────────────────────────────────────────

const ORIGINAL_THEME_KEYS: Array<keyof AppSettings> = [
  'colorBg',
  'colorSurface',
  'colorBorder',
  'colorAccent',
  'colorText',
  'colorTextMuted',
  'bgTexture',
  'fontFamily',
];

const ORIGINAL_THEME = Object.fromEntries(
  ORIGINAL_THEME_KEYS.map((key) => [key, DEFAULT_SETTINGS[key]])
) as Partial<AppSettings>;

/** THEME PRESETS is part of this module's public API. */
export const THEME_PRESETS: { label: string; settings: Partial<AppSettings> }[] = [
  {
    label: 'Dark (default)',
    settings: {
      colorBg: '#09090b', colorSurface: '#111113', colorBorder: '#27272a',
      colorAccent: '#6366f1', colorText: '#e4e4e7', colorTextMuted: '#71717a',
    },
  },
  {
    label: 'Midnight Blue',
    settings: {
      colorBg: '#0a0f1e', colorSurface: '#0f172a', colorBorder: '#1e293b',
      colorAccent: '#38bdf8', colorText: '#e2e8f0', colorTextMuted: '#64748b',
    },
  },
  {
    label: 'Forest',
    settings: {
      colorBg: '#0a130a', colorSurface: '#0f1f0f', colorBorder: '#1a3a1a',
      colorAccent: '#4ade80', colorText: '#dcfce7', colorTextMuted: '#4b7a4b',
    },
  },
  {
    label: 'Warm Dark',
    settings: {
      colorBg: '#1a1208', colorSurface: '#241b0f', colorBorder: '#3d2e1a',
      colorAccent: '#f59e0b', colorText: '#fef3c7', colorTextMuted: '#78614a',
    },
  },
  {
    label: 'Vintage Radio',
    settings: {
      colorBg: '#6a472f', colorSurface: '#261a12', colorBorder: '#7d5a3c',
      colorAccent: '#d4a15e', colorText: '#f6e4c7', colorTextMuted: '#be9a72',
      bgTexture: 'wood',
      fontFamily: 'IBM Plex Mono',
    },
  },
  {
    label: 'Light',
    settings: {
      colorBg: '#f8f8f8', colorSurface: '#ffffff', colorBorder: '#e4e4e7',
      colorAccent: '#6366f1', colorText: '#18181b', colorTextMuted: '#71717a',
    },
  },
  {
    label: 'Solarized',
    settings: {
      colorBg: '#002b36', colorSurface: '#073642', colorBorder: '#124652',
      colorAccent: '#268bd2', colorText: '#839496', colorTextMuted: '#586e75',
    },
  },
  {
    label: 'Neon Groove',
    settings: {
      colorBg: '#0A0A14', colorSurface: '#121225', colorBorder: '#2A2A4A',
      colorAccent: '#FF4FD8', colorText: '#F5F7FF', colorTextMuted: '#9CA3C7',
      fontFamily: 'Fira Code',
    },
  },
  {
    label: 'Disco Citrus',
    settings: {
      colorBg: '#151008', colorSurface: '#1F1710', colorBorder: '#3A2A1A',
      colorAccent: '#00D1B2', colorText: '#FDF3DE', colorTextMuted: '#B89C78',
      fontFamily: 'Source Code Pro',
    },
  },
  {
    label: 'Ivory Ledger',
    settings: {
      colorBg: '#F6F3EC', colorSurface: '#FFFFFF', colorBorder: '#D8D0C2',
      colorAccent: '#3A5A98', colorText: '#1F2937', colorTextMuted: '#6B7280',
      fontFamily: 'Inter',
    },
  },
  {
    label: 'Oxford Brass',
    settings: {
      colorBg: '#0F172A', colorSurface: '#111C33', colorBorder: '#24324D',
      colorAccent: '#C8A76A', colorText: '#E5E7EB', colorTextMuted: '#94A3B8',
      fontFamily: 'IBM Plex Mono',
    },
  },
  {
    label: 'Graphite Mint',
    settings: {
      colorBg: '#0B0F12', colorSurface: '#11181F', colorBorder: '#24303A',
      colorAccent: '#22C55E', colorText: '#E6EDF3', colorTextMuted: '#8B9BB0',
      fontFamily: 'JetBrains Mono',
    },
  },
];

// ─── Frequency options ────────────────────────────────────────────────────────

const FREQ_OPTIONS = [
  { label: '30 minutes',  value: 0.5 },
  { label: '1 hour',      value: 1 },
  { label: '2 hours',     value: 2 },
  { label: '4 hours',     value: 4 },
  { label: '6 hours',     value: 6 },
  { label: '12 hours',    value: 12 },
  { label: 'Daily',       value: 24 },
  { label: 'Every 2 days', value: 48 },
  { label: 'Weekly',      value: 168 },
];

const WAVEFORM_BATCH_OPTIONS = [25, 50, 100, 250, 500, 1000];

// ─── Schedule Row ─────────────────────────────────────────────────────────────

function ScheduleRow({
  library,
  schedule,
  onSave,
  onDelete,
}: {
  library: Library;
  schedule: ScanSchedule | null;
  onSave: (libraryId: ClientEntityId, enabled: boolean, freq: number) => Promise<void>;
  onDelete: (libraryId: ClientEntityId) => Promise<void>;
}) {
  const [enabled, setEnabled] = useState(schedule ? Boolean(schedule.enabled) : false);
  const [freq, setFreq] = useState(schedule?.frequency_hours ?? 24);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    if (schedule) {
      setEnabled(Boolean(schedule.enabled));
      setFreq(schedule.frequency_hours);
    }
  }, [schedule]);

  const save = async () => {
    setSaving(true);
    await onSave(library.id, enabled, freq);
    setSaving(false);
    setDirty(false);
  };

  const toggle = (val: boolean) => { setEnabled(val); setDirty(true); };
  const changeFreq = (val: number) => { setFreq(val); setDirty(true); };

  return (
    <div style={Sc.row}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 600, fontSize: 13, color: 'var(--text)', marginBottom: 2 }}>
          {library.name}
        </div>
        <div style={{ fontSize: 11, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {library.primary_path ?? library.path}
        </div>
        {schedule && (
          <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 4, display: 'flex', gap: 16 }}>
            <span>Last: {fmtLastRun(schedule.last_run)}</span>
            {schedule.enabled ? <span>Next: {fmtNextRun(schedule.next_run)}</span> : null}
          </div>
        )}
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexShrink: 0 }}>
        {/* Enable toggle */}
        <label style={{ display: 'flex', alignItems: 'center', gap: 6, cursor: 'pointer', fontSize: 12, color: 'var(--text-muted)' }}>
          <div
            onClick={() => toggle(!enabled)}
            style={{
              width: 36, height: 20, borderRadius: 10, cursor: 'pointer', position: 'relative',
              backgroundColor: enabled ? 'var(--accent)' : 'var(--border)', transition: 'background 0.2s',
            }}
          >
            <div style={{
              position: 'absolute', top: 2, left: enabled ? 18 : 2,
              width: 16, height: 16, borderRadius: '50%', backgroundColor: '#fff',
              transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.3)',
            }} />
          </div>
          {enabled ? 'On' : 'Off'}
        </label>

        {/* Frequency */}
        <select
          value={freq}
          onChange={e => changeFreq(Number(e.target.value))}
          disabled={!enabled}
          style={{
            background: 'var(--surface)', border: '1px solid var(--border)', color: enabled ? 'var(--text)' : 'var(--text-muted)',
            borderRadius: 6, padding: '5px 8px', fontSize: 12, fontFamily: 'inherit', outline: 'none',
            opacity: enabled ? 1 : 0.4,
          }}
        >
          {FREQ_OPTIONS.map(o => (
            <option key={o.value} value={o.value}>{o.label}</option>
          ))}
        </select>

        {/* Save */}
        {dirty && (
          <button onClick={save} disabled={saving} style={Sc.saveBtn}>
            {saving ? '…' : 'Save'}
          </button>
        )}
        {!dirty && schedule && (
          <button onClick={() => onDelete(library.id)} style={Sc.deleteBtn} title="Remove schedule">
            ✕
          </button>
        )}
      </div>
    </div>
  );
}

const Sc = {
  row: {
    display: 'flex', alignItems: 'center', gap: 16,
    padding: '14px 16px', borderRadius: 8, marginBottom: 8,
    backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
  } as React.CSSProperties,
  saveBtn: {
    background: 'var(--accent)', color: '#fff', border: 'none',
    borderRadius: 6, padding: '5px 12px', cursor: 'pointer',
    fontSize: 12, fontFamily: 'inherit', fontWeight: 600,
  } as React.CSSProperties,
  deleteBtn: {
    background: 'transparent', color: 'var(--text-muted)', border: '1px solid var(--border)',
    borderRadius: 6, padding: '4px 8px', cursor: 'pointer', fontSize: 11, fontFamily: 'inherit',
  } as React.CSSProperties,
};

// ─── Main Settings Page ───────────────────────────────────────────────────────

interface Props {
  currentUser: AuthUser;
  onLogout: () => void;
  settings: AppSettings;
  libraries?: Library[];
  onLibrariesRefresh?: () => Promise<void>;
  onSettingsChange: (s: AppSettings) => void;
  onStreamDirectChange?: (val: boolean) => void;
  adaptiveAccentEnabled?: boolean;
  onAdaptiveAccentEnabledChange?: (enabled: boolean) => void;
  playbackMode?: 'standard' | 'vinyl';
  onPlaybackModeChange?: (mode: 'standard' | 'vinyl') => void;
  vinylHardcore?: boolean;
  onVinylHardcoreChange?: (enabled: boolean) => void;
  vinylNeedleDrop?: boolean;
  onVinylNeedleDropChange?: (enabled: boolean) => void;
  vinylAnalogFxDisabled?: boolean;
  onVinylAnalogFxDisabledChange?: (enabled: boolean) => void;
  vinylNeedleDropIntensity?: number;
  onVinylNeedleDropIntensityChange?: (value: number) => void;
}

/** Has Spotify Credentials is part of this module's public API. */
export function hasSpotifyCredentials(clientId: string, clientSecret: string): boolean {
  return Boolean(clientId.trim() && clientSecret.trim());
}

/** Is Valid DLNA Port is part of this module's public API. */
export function isValidDlnaPort(rawPort: string): boolean {
  const parsed = Number(rawPort.trim());
  return Number.isInteger(parsed) && parsed >= 1024 && parsed <= 65535;
}

/** Settings Page is part of this module's public API. */
export default function SettingsPage({
  currentUser,
  onLogout,
  settings,
  libraries: appLibraries,
  onLibrariesRefresh,
  onSettingsChange,
  onStreamDirectChange,
  adaptiveAccentEnabled = true,
  onAdaptiveAccentEnabledChange,
  playbackMode = 'standard',
  onPlaybackModeChange,
  vinylHardcore = false,
  onVinylHardcoreChange,
  vinylNeedleDrop = false,
  onVinylNeedleDropChange,
  vinylAnalogFxDisabled = false,
  onVinylAnalogFxDisabledChange,
  vinylNeedleDropIntensity = 0.65,
  onVinylNeedleDropIntensityChange,
}: Props) {
  const isAdmin = currentUser.role === 'admin';
  const [local, setLocal] = useState<AppSettings>(settings);
  const [activeTab, setActiveTab] = useState<'theme' | 'libraries' | 'about' | 'schedules' | 'integrations' | 'advanced' | 'users'>('theme');
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [schedules, setSchedules] = useState<ScanSchedule[]>([]);
  const [queueSnapshot, setQueueSnapshot] = useState<AdminQueueSnapshot | null>(null);
  const [queueLoading, setQueueLoading] = useState(false);
  const [queueError, setQueueError] = useState<string | null>(null);
  const [queueActionBusy, setQueueActionBusy] = useState<string | null>(null);
  const [queueActionResult, setQueueActionResult] = useState<string | null>(null);
  const [showRawQueueSnapshot, setShowRawQueueSnapshot] = useState(false);
  const [providerUsage, setProviderUsage] = useState<ProviderUsageSnapshot | null>(null);
  const [providerUsageLoading, setProviderUsageLoading] = useState(false);
  const [providerUsageError, setProviderUsageError] = useState<string | null>(null);
  const [showProviderUsageRows, setShowProviderUsageRows] = useState(false);
  const [discogsToken, setDiscogsToken] = useState('');
  const [discogsTestResult, setDiscogsTestResult] = useState<string | null>(null);
  const [discogsSaving, setDiscogsSaving] = useState(false);
  const [spotifyClientId, setSpotifyClientId] = useState('');
  const [spotifyClientSecret, setSpotifyClientSecret] = useState('');
  const [spotifySaving, setSpotifySaving] = useState(false);
  const [spotifyResult, setSpotifyResult] = useState<string | null>(null);
  const [geniusClientId, setGeniusClientId] = useState('');
  const [geniusClientSecret, setGeniusClientSecret] = useState('');
  const [geniusSaving, setGeniusSaving] = useState(false);
  const [geniusResult, setGeniusResult] = useState<string | null>(null);
  const [lastfmKey, setLastfmKey] = useState('');
  const [lastfmSaving, setLastfmSaving] = useState(false);
  const [lastfmResult, setLastfmResult] = useState<string | null>(null);
  const [streamDirect, setStreamDirectState] = useState(() => getStreamDirect());
  const [transcodeQuality, setTranscodeQuality] = useState<'low' | 'high'>(settings.transcodeQuality === 'high' ? 'high' : 'low');
  const [transcodeQualitySaving, setTranscodeQualitySaving] = useState(false);
  const [transcodeQualityResult, setTranscodeQualityResult] = useState<string | null>(null);
  const [replayGainEnabled, setReplayGainEnabled] = useState(settings.replayGainEnabled === 'true');
  const [defaultVinylMode, setDefaultVinylMode] = useState(settings.vinylMode === 'vinyl');

  // DLNA
  const [dlnaEnabled, setDlnaEnabled] = useState(settings.dlnaEnabled === 'true');
  const [dlnaFriendlyName, setDlnaFriendlyName] = useState(settings.dlnaFriendlyName || 'BoogieBox');
  const [dlnaPort, setDlnaPort] = useState(settings.dlnaPort || '8200');
  const [dlnaStatus, setDlnaStatus] = useState<{ running: boolean; port: number | null; friendlyName: string | null } | null>(null);
  const [dlnaSaving, setDlnaSaving] = useState(false);
  const [dlnaResult, setDlnaResult] = useState<string | null>(null);

  // Crossfade
  const [cfMode, setCfMode] = useState<string>(settings.crossfadeMode || 'off');
  const [cfDuration, setCfDuration] = useState(Number(settings.crossfadeDuration) || 2);
  const [cfSaving, setCfSaving] = useState(false);
  const [cfResult, setCfResult] = useState<string | null>(null);
  const [waveformGenerateOnMissing, setWaveformGenerateOnMissing] = useState(settings.waveformGenerateOnMissing !== 'false');
  const [waveformBackgroundEnabled, setWaveformBackgroundEnabled] = useState(settings.waveformBackgroundEnabled === 'true');
  const [waveformFrequencyHours, setWaveformFrequencyHours] = useState(Number(settings.waveformBackgroundFrequencyHours) || 24);
  const [waveformBatchSize, setWaveformBatchSize] = useState(Number(settings.waveformBackgroundBatchSize) || 100);
  const [waveformStatus, setWaveformStatus] = useState<WaveformMappingStatus | null>(null);
  const [waveformLoading, setWaveformLoading] = useState(false);
  const [waveformSaving, setWaveformSaving] = useState(false);
  const [waveformSettingsResult, setWaveformSettingsResult] = useState<string | null>(null);
  const [waveformRunResult, setWaveformRunResult] = useState<string | null>(null);
  const [bpmStatus, setBpmStatus] = useState<BpmAnalysisStatus | null>(null);
  const [bpmBackgroundEnabled, setBpmBackgroundEnabled] = useState(settings.bpmBackgroundEnabled === 'true');
  const [bpmFrequencyHours, setBpmFrequencyHours] = useState(Number(settings.bpmBackgroundFrequencyHours) || 24);
  const [bpmLoading, setBpmLoading] = useState(false);
  const [bpmSaving, setBpmSaving] = useState(false);
  const [bpmSettingsResult, setBpmSettingsResult] = useState<string | null>(null);
  const [bpmRunResult, setBpmRunResult] = useState<string | null>(null);
  const [scanDebugLoggingEnabled, setScanDebugLoggingEnabled] = useState(settings.scanDebugLoggingEnabled === 'true');
  const [scanDebugSaving, setScanDebugSaving] = useState(false);
  const [scanDebugResult, setScanDebugResult] = useState<string | null>(null);
  const [boogiemixOutputFolder, setBoogiemixOutputFolder] = useState(settings.boogiemixOutputFolder || '');
  const [boogiemixOutputFolderSaving, setBoogiemixOutputFolderSaving] = useState(false);
  const [boogiemixOutputFolderResult, setBoogiemixOutputFolderResult] = useState<string | null>(null);
  const [boogiemixDeepStatus, setBoogiemixDeepStatus] = useState<BoogieMixDeepAnalysisStatus | null>(null);
  const [boogiemixDeepLoading, setBoogiemixDeepLoading] = useState(false);
  const [boogiemixDeepBackgroundMode, setBoogiemixDeepBackgroundMode] = useState(settings.boogiemixDeepAnalysisBackgroundMode || 'off');
  const [boogiemixDeepPauseBackground, setBoogiemixDeepPauseBackground] = useState(settings.boogiemixDeepAnalysisPauseBackground === 'true');
  const [boogiemixDeepMaxDurationMins, setBoogiemixDeepMaxDurationMins] = useState(Number(settings.boogiemixDeepAnalysisMaxDurationMins) || 15);
  const [boogiemixDeepActionBusy, setBoogiemixDeepActionBusy] = useState<string | null>(null);
  const [boogiemixDeepActionResult, setBoogiemixDeepActionResult] = useState<string | null>(null);
  const [boogiemixDeepSelectedLibrary, setBoogiemixDeepSelectedLibrary] = useState<ClientEntityId | ''>('');
  const showGeniusIntegration = false;
  const [currentDbFolder, setCurrentDbFolder] = useState<string>('');
  const [switchDbFolder, setSwitchDbFolder] = useState<string>('');
  const [switchDbSaving, setSwitchDbSaving] = useState(false);
  const [switchDbResult, setSwitchDbResult] = useState<string | null>(null);
  const [showDbFolderPicker, setShowDbFolderPicker] = useState(false);

  useEffect(() => {
    setLocal(settings);
    setCfMode(settings.crossfadeMode || 'off');
    setCfDuration(Number(settings.crossfadeDuration) || 2);
    setTranscodeQuality(settings.transcodeQuality === 'high' ? 'high' : 'low');
    setReplayGainEnabled(settings.replayGainEnabled === 'true');
    setDefaultVinylMode(settings.vinylMode === 'vinyl');
    setWaveformGenerateOnMissing(settings.waveformGenerateOnMissing !== 'false');
    setWaveformBackgroundEnabled(settings.waveformBackgroundEnabled === 'true');
    setWaveformFrequencyHours(Number(settings.waveformBackgroundFrequencyHours) || 24);
    setWaveformBatchSize(Number(settings.waveformBackgroundBatchSize) || 100);
    setBpmBackgroundEnabled(settings.bpmBackgroundEnabled === 'true');
    setBpmFrequencyHours(Number(settings.bpmBackgroundFrequencyHours) || 24);
    setScanDebugLoggingEnabled(settings.scanDebugLoggingEnabled === 'true');
    setBoogiemixOutputFolder(settings.boogiemixOutputFolder || '');
    setBoogiemixDeepBackgroundMode(settings.boogiemixDeepAnalysisBackgroundMode || 'off');
    setBoogiemixDeepPauseBackground(settings.boogiemixDeepAnalysisPauseBackground === 'true');
    setBoogiemixDeepMaxDurationMins(Number(settings.boogiemixDeepAnalysisMaxDurationMins) || 15);
  }, [settings]);

  const loadSchedules = useCallback(async () => {
    const [libs, scheds] = await Promise.all([
      api.libraries.list(),
      api.schedules.list(),
    ]);
    setLibraries(libs);
    setSchedules(scheds);
  }, []);

  const refreshLibraries = useCallback(async () => {
    if (onLibrariesRefresh) {
      await onLibrariesRefresh();
    }
    const libs = await api.libraries.list();
    setLibraries(libs);
  }, [onLibrariesRefresh]);

  const loadQueueSnapshot = useCallback(async () => {
    setQueueLoading(true);
    setQueueError(null);
    try {
      setQueueSnapshot(await api.admin.queues());
    } catch (e: any) {
      setQueueError(e.message ?? 'Failed to load queues');
    } finally {
      setQueueLoading(false);
    }
  }, []);

  const loadProviderUsage = useCallback(async () => {
    if (!isAdmin) return;
    setProviderUsageLoading(true);
    setProviderUsageError(null);
    try {
      setProviderUsage(await api.admin.providerUsage());
    } catch (e: any) {
      setProviderUsageError(e.message ?? 'Failed to load provider usage');
    } finally {
      setProviderUsageLoading(false);
    }
  }, [isAdmin]);

  const runQueueAction = useCallback(async (busyKey: string, successMessage: string, action: () => Promise<unknown>) => {
    setQueueActionBusy(busyKey);
    setQueueActionResult(null);
    setQueueError(null);
    try {
      await action();
      await loadQueueSnapshot();
      setQueueActionResult(successMessage);
    } catch (e: any) {
      setQueueError(e.message ?? 'Queue action failed');
    } finally {
      setQueueActionBusy(null);
    }
  }, [loadQueueSnapshot]);

  const loadDlnaStatus = useCallback(async () => {
    try {
      const status = await api.dlna.status();
      setDlnaStatus(status);
    } catch { setDlnaStatus(null); }
  }, []);

  const loadWaveformStatus = useCallback(async () => {
    setWaveformLoading(true);
    try {
      const status = await api.waveforms.status();
      setWaveformStatus(status);
      setWaveformGenerateOnMissing(status.generateOnMissing);
      setWaveformBackgroundEnabled(status.enabled);
      setWaveformFrequencyHours(status.frequencyHours);
      setWaveformBatchSize(status.batchSize);
    } catch {
      setWaveformStatus(null);
    } finally {
      setWaveformLoading(false);
    }
  }, []);

  const loadBpmStatus = useCallback(async () => {
    setBpmLoading(true);
    try {
      const status = await api.bpm.status();
      setBpmStatus(status);
      setBpmBackgroundEnabled(status.backgroundEnabled);
      setBpmFrequencyHours(status.frequencyHours);
    } catch {
      setBpmStatus(null);
    } finally {
      setBpmLoading(false);
    }
  }, []);

  const loadBoogieMixDeepStatus = useCallback(async (showSpinner = false) => {
    if (showSpinner) setBoogiemixDeepLoading(true);
    try {
      const status = await api.boogiemix.deepAnalysisStatus();
      setBoogiemixDeepStatus(status);
      if (status.controls) {
        setBoogiemixDeepBackgroundMode(status.controls.backgroundMode);
        setBoogiemixDeepPauseBackground(status.controls.pauseBackground);
      }
    } catch {
      setBoogiemixDeepStatus(null);
    } finally {
      if (showSpinner) setBoogiemixDeepLoading(false);
    }
  }, []);

  useEffect(() => {
    if (appLibraries) setLibraries(appLibraries);
  }, [appLibraries]);

  useEffect(() => {
    if (activeTab === 'libraries') refreshLibraries().catch(() => {});
    if (activeTab === 'schedules') loadSchedules();
    if (activeTab === 'schedules' && isAdmin) loadQueueSnapshot();
    if (activeTab === 'integrations') {
      api.settings.get().then(s => {
        if (s.discogsToken) setDiscogsToken(s.discogsToken);
        if (s.spotifyClientId) setSpotifyClientId(s.spotifyClientId);
        if (s.spotifyClientSecret) setSpotifyClientSecret(s.spotifyClientSecret);
        if (s.geniusClientId) setGeniusClientId(s.geniusClientId);
        if (s.geniusClientSecret) setGeniusClientSecret(s.geniusClientSecret);
        if (s.lastfmKey) setLastfmKey(s.lastfmKey);
      });
      if (isAdmin) loadProviderUsage();
    }
    if (activeTab === 'advanced') {
      api.settings.get().then(s => {
        setDlnaEnabled(s.dlnaEnabled === 'true');
        setDlnaFriendlyName(s.dlnaFriendlyName || 'BoogieBox');
        setDlnaPort(s.dlnaPort || '8200');
        setTranscodeQuality((s.transcodeQuality ?? 'low') === 'high' ? 'high' : 'low');
        setReplayGainEnabled((s.replayGainEnabled ?? 'false') === 'true');
        setDefaultVinylMode((s.vinylMode ?? 'standard') === 'vinyl');
        setWaveformGenerateOnMissing((s.waveformGenerateOnMissing ?? 'true') === 'true');
        setWaveformBackgroundEnabled((s.waveformBackgroundEnabled ?? 'false') === 'true');
        setWaveformFrequencyHours(Number(s.waveformBackgroundFrequencyHours ?? '24') || 24);
        setWaveformBatchSize(Number(s.waveformBackgroundBatchSize ?? '100') || 100);
        setBpmBackgroundEnabled((s.bpmBackgroundEnabled ?? 'false') === 'true');
        setBpmFrequencyHours(Number(s.bpmBackgroundFrequencyHours ?? '24') || 24);
        setScanDebugLoggingEnabled((s.scanDebugLoggingEnabled ?? 'false') === 'true');
        setBoogiemixOutputFolder(s.boogiemixOutputFolder ?? '');
        setBoogiemixDeepBackgroundMode(s.boogiemixDeepAnalysisBackgroundMode ?? 'off');
        setBoogiemixDeepPauseBackground((s.boogiemixDeepAnalysisPauseBackground ?? 'false') === 'true');
        setBoogiemixDeepMaxDurationMins(Number(s.boogiemixDeepAnalysisMaxDurationMins ?? '15') || 15);
      });
      refreshLibraries().catch(() => {});
      loadDlnaStatus();
      loadWaveformStatus();
      loadBpmStatus();
      loadBoogieMixDeepStatus(true);
      api.systemStatus().then(s => { if (s.dbFolder) setCurrentDbFolder(s.dbFolder); }).catch(() => {});
    }
  }, [activeTab, isAdmin, refreshLibraries, loadSchedules, loadQueueSnapshot, loadDlnaStatus, loadWaveformStatus, loadBpmStatus, loadBoogieMixDeepStatus, loadProviderUsage]);

  useEffect(() => {
    if (activeTab !== 'advanced') return;
    const isRunning = (boogiemixDeepStatus?.queue.running ?? 0) > 0 || (boogiemixDeepStatus?.queue.pending ?? 0) > 0;
    const interval = setInterval(() => { loadBoogieMixDeepStatus(); }, isRunning ? 3000 : 10000);
    return () => clearInterval(interval);
  }, [activeTab, boogiemixDeepStatus?.queue.running, boogiemixDeepStatus?.queue.pending, loadBoogieMixDeepStatus]);

  const set = (key: keyof AppSettings, value: string) => {
    const next = { ...local, [key]: value };
    setLocal(next);
    onSettingsChange(next); // live preview
  };

  const applyPreset = (preset: Partial<AppSettings>) => {
    const next = { ...local, bgTexture: 'none', ...preset };
    setLocal(next);
    onSettingsChange(next);
  };

  const resetTheme = () => {
    const next = { ...local, ...ORIGINAL_THEME };
    setLocal(next);
    onSettingsChange(next);
  };

  const saveSchedule = async (libraryId: ClientEntityId, enabled: boolean, freq: number) => {
    await api.schedules.upsert(libraryId, enabled, freq);
    await loadSchedules();
  };

  const deleteSchedule = async (libraryId: ClientEntityId) => {
    await api.schedules.remove(libraryId);
    await loadSchedules();
  };

  const getScheduleFor = (libraryId: ClientEntityId) =>
    schedules.find(s => s.library_id === libraryId) ?? null;

  const saveWaveformSettings = async (updates: Record<string, string>) => {
    setWaveformSaving(true);
    setWaveformSettingsResult(null);
    try {
      await api.settings.update(updates);
      await loadWaveformStatus();
      setWaveformSettingsResult('Saved');
    } catch (e: any) {
      setWaveformSettingsResult(`Error: ${e.message}`);
    } finally {
      setWaveformSaving(false);
      setTimeout(() => setWaveformSettingsResult(null), 2500);
    }
  };

  const runBoogieMixDeepAction = async (busyKey: string, action: () => Promise<string>) => {
    setBoogiemixDeepActionBusy(busyKey);
    setBoogiemixDeepActionResult(null);
    try {
      const message = await action();
      await loadBoogieMixDeepStatus();
      setBoogiemixDeepActionResult(message);
    } catch (e: any) {
      setBoogiemixDeepActionResult(`Error: ${e.message ?? 'BoogieMix deep-analysis action failed'}`);
    } finally {
      setBoogiemixDeepActionBusy(null);
    }
  };

  const runWaveformMappingNow = async () => {
    setWaveformRunResult('Running...');
    try {
      const run = await api.waveforms.runMap();
      setWaveformRunResult(
        `Done: processed ${run.processed}, generated ${run.generated}, skipped ${run.skipped}, errors ${run.errors}`,
      );
      await loadWaveformStatus();
    } catch (e: any) {
      setWaveformRunResult(`Error: ${e.message}`);
    }
  };

  const saveBpmSettings = async (updates: Record<string, string>) => {
    setBpmSaving(true);
    setBpmSettingsResult(null);
    try {
      await api.settings.update(updates);
      await loadBpmStatus();
      setBpmSettingsResult('Saved');
    } catch (e: any) {
      setBpmSettingsResult(`Error: ${e.message}`);
    } finally {
      setBpmSaving(false);
      setTimeout(() => setBpmSettingsResult(null), 2500);
    }
  };

  const saveScanDebugSettings = async (enabled: boolean) => {
    setScanDebugSaving(true);
    setScanDebugResult(null);
    try {
      await api.settings.update({ scanDebugLoggingEnabled: enabled ? 'true' : 'false' });
      setScanDebugLoggingEnabled(enabled);
      setScanDebugResult(enabled ? 'Debug logging enabled' : 'Debug logging disabled');
    } catch (e: any) {
      setScanDebugLoggingEnabled(!enabled);
      setScanDebugResult(`Error: ${e.message}`);
    } finally {
      setScanDebugSaving(false);
      setTimeout(() => setScanDebugResult(null), 3000);
    }
  };

  const saveDiscogsToken = async () => {
    setDiscogsSaving(true);
    setDiscogsTestResult(null);
    try {
      await api.settings.update({ discogsToken: discogsToken.trim() });
      setDiscogsTestResult('✓ Token saved');
      setTimeout(() => setDiscogsTestResult(null), 3000);
    } catch (e: any) {
      setDiscogsTestResult('✗ ' + e.message);
    } finally {
      setDiscogsSaving(false);
    }
  };

  const testDiscogsToken = async () => {
    if (!discogsToken.trim()) { setDiscogsTestResult('Enter a token first'); return; }
    setDiscogsSaving(true);
    setDiscogsTestResult('Testing…');
    try {
      const resp = await fetch(
        `https://api.discogs.com/database/search?q=test&per_page=1`,
        { headers: { Authorization: `Discogs token=${discogsToken.trim()}`, 'User-Agent': 'BoogieBox/1.0' } }
      );
      if (resp.ok) {
        setDiscogsTestResult('✓ Token is valid — Discogs connection OK');
      } else {
        const j = await resp.json().catch(() => ({}));
        setDiscogsTestResult(`✗ Discogs returned ${resp.status}: ${(j as any).message ?? 'error'}`);
      }
    } catch (e: any) {
      setDiscogsTestResult('✗ Network error: ' + e.message);
    } finally {
      setDiscogsSaving(false);
    }
  };

  const saveSpotifyCreds = async () => {
    setSpotifySaving(true);
    setSpotifyResult(null);
    try {
      await api.settings.update({
        spotifyClientId: spotifyClientId.trim(),
        spotifyClientSecret: spotifyClientSecret.trim(),
      });
      setSpotifyResult('✓ Spotify credentials saved');
      setTimeout(() => setSpotifyResult(null), 3000);
    } catch (e: any) {
      setSpotifyResult('✗ ' + e.message);
    } finally {
      setSpotifySaving(false);
    }
  };

  const testSpotifyCreds = async () => {
    if (!hasSpotifyCredentials(spotifyClientId, spotifyClientSecret)) {
      setSpotifyResult('Enter client ID and client secret first');
      return;
    }
    setSpotifySaving(true);
    setSpotifyResult('Testing…');
    try {
      await api.integrations.spotifyTest();
      setSpotifyResult('✓ Spotify connection OK');
    } catch (e: any) {
      setSpotifyResult('✗ ' + e.message);
    } finally {
      setSpotifySaving(false);
    }
  };

  const saveGeniusCreds = async () => {
    setGeniusSaving(true);
    setGeniusResult(null);
    try {
      await api.settings.update({
        geniusClientId: geniusClientId.trim(),
        geniusClientSecret: geniusClientSecret.trim(),
      });
      setGeniusResult('OK Genius credentials saved');
      setTimeout(() => setGeniusResult(null), 3000);
    } catch (e: any) {
      setGeniusResult('Error: ' + e.message);
    } finally {
      setGeniusSaving(false);
    }
  };

  const testGeniusCreds = async () => {
    if (!hasSpotifyCredentials(geniusClientId, geniusClientSecret)) {
      setGeniusResult('Enter client ID and client secret first');
      return;
    }
    setGeniusSaving(true);
    setGeniusResult('Testing...');
    try {
      await api.integrations.geniusTest(geniusClientId.trim(), geniusClientSecret.trim());
      setGeniusResult('OK Genius connection OK');
    } catch (e: any) {
      setGeniusResult('Error: ' + e.message);
    } finally {
      setGeniusSaving(false);
    }
  };

  const saveLastfmKey = async () => {
    setLastfmSaving(true);
    setLastfmResult(null);
    try {
      await api.settings.update({ lastfmKey: lastfmKey.trim() });
      onSettingsChange({ ...local, lastfmKey: lastfmKey.trim() } as any);
      setLastfmResult('✓ API key saved');
      setTimeout(() => setLastfmResult(null), 3000);
    } catch (e: any) {
      setLastfmResult('✗ ' + e.message);
    } finally {
      setLastfmSaving(false);
    }
  };

  const testLastfmKey = async () => {
    if (!lastfmKey.trim()) { setLastfmResult('Enter a key first'); return; }
    setLastfmSaving(true);
    setLastfmResult('Testing…');
    try {
      const url = `https://ws.audioscrobbler.com/2.0/?method=chart.gettopartists&api_key=${encodeURIComponent(lastfmKey.trim())}&format=json&limit=1`;
      const resp = await fetch(url);
      const data = await resp.json();
      if (data.error) {
        setLastfmResult(`✗ Last.fm error ${data.error}: ${data.message}`);
      } else {
        setLastfmResult('✓ Key is valid — Last.fm connection OK');
      }
    } catch (e: any) {
      setLastfmResult('✗ Network error: ' + e.message);
    } finally {
      setLastfmSaving(false);
    }
  };

  return (
    <div style={P.page}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 4 }}>
        <h2 style={{ ...P.title, margin: 0 }}>Settings</h2>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 12, color: 'var(--text-muted)' }}>
          <span>{currentUser.username}</span>
          <span style={{ padding: '2px 6px', borderRadius: 4, background: isAdmin ? 'rgba(99,102,241,0.15)' : 'rgba(255,255,255,0.06)', color: isAdmin ? 'var(--accent)' : 'var(--text-muted)', fontWeight: 600, fontSize: 10 }}>{currentUser.role}</span>
          <button onClick={onLogout} style={{ background: 'transparent', border: '1px solid var(--border)', color: 'var(--text-muted)', borderRadius: 6, padding: '4px 10px', cursor: 'pointer', fontSize: 11 }}>Log out</button>
        </div>
      </div>

      {/* Tab bar */}
      <div style={P.tabBar}>
        {([
          ['theme',        '🎨 Appearance'],
          ['libraries',    '📚 Libraries'],
          ...(isAdmin ? [
            ['schedules',    '🕐 Auto-Scan'],
            ['integrations', '🔌 Integrations'],
            ['advanced',     '⚙ Advanced'],
            ['users',        '👥 Users'],
          ] : []),
          ['about',        'About'],
        ] as [string, string][]).map(([t, label]) => (
          <button
            key={t}
            style={{ ...P.tab, ...(activeTab === t ? P.tabActive : {}) }}
            onClick={() => setActiveTab(t as any)}
          >
            {label}
          </button>
        ))}
      </div>

      {/* ── Theme Tab ─────────────────────────────────────────────────────── */}
      {activeTab === 'libraries' && (
        <div style={P.section}>
          <LibrarySettingsTab libraries={libraries as Library[]} onRefresh={refreshLibraries} />
        </div>
      )}

      {activeTab === 'theme' && (
        <div style={P.section}>

          {/* Presets */}
          <div style={P.sectionTitle}>Presets</div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 28 }}>
            {THEME_PRESETS.map(preset => (
              <button
                key={preset.label}
                onClick={() => applyPreset(preset.settings)}
                style={{
                  padding: '6px 14px', borderRadius: 6, fontSize: 12, cursor: 'pointer',
                  border: '1px solid var(--border)', fontFamily: 'inherit',
                  background: preset.settings.colorBg ?? 'var(--surface)',
                  color: preset.settings.colorText ?? 'var(--text)',
                  transition: 'opacity 0.15s',
                }}
              >
                {preset.label}
              </button>
            ))}
          </div>

          {/* Colors */}
          <div style={P.sectionTitle}>Colors</div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 40px', marginBottom: 28 }}>
            <ColorInput label="Background"    value={local.colorBg}        onChange={v => set('colorBg', v)} />
            <ColorInput label="Surface"       value={local.colorSurface}   onChange={v => set('colorSurface', v)} />
            <ColorInput label="Border"        value={local.colorBorder}    onChange={v => set('colorBorder', v)} />
            <ColorInput label="Accent"        value={local.colorAccent}    onChange={v => set('colorAccent', v)} />
            <ColorInput label="Text"          value={local.colorText}      onChange={v => set('colorText', v)} />
            <ColorInput label="Muted Text"    value={local.colorTextMuted} onChange={v => set('colorTextMuted', v)} />
          </div>

          {/* Font */}
          <div style={P.sectionTitle}>Font</div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 28 }}>
            {FONT_OPTIONS.map(f => (
              <button
                key={f.value}
                onClick={() => set('fontFamily', f.value)}
                style={{
                  padding: '7px 14px', borderRadius: 6, fontSize: 13, cursor: 'pointer',
                  fontFamily: f.value,
                  border: `1px solid ${local.fontFamily === f.value ? 'var(--accent)' : 'var(--border)'}`,
                  backgroundColor: local.fontFamily === f.value ? 'color-mix(in srgb, var(--accent) 15%, transparent)' : 'var(--surface)',
                  color: local.fontFamily === f.value ? 'var(--accent)' : 'var(--text)',
                }}
              >
                {f.label}
              </button>
            ))}
          </div>

          {/* Preview */}
          <div style={P.sectionTitle}>Preview</div>
          <div style={{
            padding: 20, borderRadius: 8, marginBottom: 24,
            backgroundColor: local.colorSurface, border: `1px solid ${local.colorBorder}`,
            fontFamily: local.fontFamily,
          }}>
            <div style={{ color: local.colorText, fontWeight: 700, fontSize: 15, marginBottom: 6 }}>
              The Quick Brown Fox
            </div>
            <div style={{ color: local.colorTextMuted, fontSize: 12, marginBottom: 10 }}>
              Artist · Album · 2024 · Jazz
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <div style={{ padding: '6px 14px', borderRadius: 6, backgroundColor: local.colorAccent, color: '#fff', fontSize: 12, fontWeight: 600 }}>
                ▶ Play
              </div>
              <div style={{ padding: '6px 14px', borderRadius: 6, border: `1px solid ${local.colorBorder}`, color: local.colorTextMuted, fontSize: 12 }}>
                + Queue
              </div>
            </div>
          </div>

          {/* Actions */}
          <div style={{ marginBottom: 16 }}>
            <div style={{ fontSize: 12, color: 'var(--text)', fontWeight: 600, marginBottom: 6 }}>
              Accent source
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div
                onClick={() => onAdaptiveAccentEnabledChange?.(!adaptiveAccentEnabled)}
                title={adaptiveAccentEnabled ? 'Adaptive accent is enabled' : 'Selected theme accent is enabled'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0,
                  backgroundColor: adaptiveAccentEnabled ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: adaptiveAccentEnabled ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
              <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.5 }}>
                {adaptiveAccentEnabled
                  ? 'Adaptive from album/artist artwork (current behavior)'
                  : 'Use selected theme accent only'}
              </div>
            </div>
            <div style={{ color: 'var(--text-muted)', fontSize: 11, marginTop: 6 }}>
              Applies to album view, artist view, and playback bar. Saved in this browser only.
            </div>
          </div>
          <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
            <div style={{ color: 'var(--text-muted)', fontSize: 11 }}>
              Theme and accent settings auto-save in this browser only.
            </div>
            <button style={P.btnSecondary} onClick={resetTheme}>
              Reset to Default
            </button>
          </div>
        </div>
      )}

      {/* ── Schedules Tab ─────────────────────────────────────────────────── */}
      {activeTab === 'schedules' && (
        <div style={P.section}>
          <div style={P.sectionTitle}>Auto-Scan Schedule</div>
          <p style={{ color: 'var(--text-muted)', fontSize: 12, marginBottom: 20, lineHeight: 1.6 }}>
            Set how often each library should be checked for new files. Scans run in the
            background and schedule changes take effect right away.
          </p>

          {libraries.length === 0 && (
            <div style={{ color: 'var(--text-muted)', fontSize: 13, padding: '20px 0' }}>
              No libraries added yet. Add a library first.
            </div>
          )}

          {libraries.map(lib => (
            <ScheduleRow
              key={lib.id}
              library={lib}
              schedule={getScheduleFor(lib.id)}
              onSave={saveSchedule}
              onDelete={deleteSchedule}
            />
          ))}

          <div style={{ marginTop: 16, padding: '12px 16px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, marginBottom: 10 }}>
              <div>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)' }}>Queue &amp; Maintenance</div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 4 }}>
                  Monitor background activity, stop stuck scan work, and queue maintenance tasks when you need them.
                </div>
              </div>
              <button onClick={() => void loadQueueSnapshot()} disabled={queueLoading} style={P.btnSecondary}>
                {queueLoading ? 'Refreshing…' : 'Refresh'}
              </button>
            </div>
            {queueActionResult && (
              <div style={{ color: '#86efac', fontSize: 11, marginBottom: 10 }}>
                {queueActionResult}
              </div>
            )}
            {queueError && (
              <div style={{ color: '#fca5a5', fontSize: 11, marginBottom: 10 }}>
                {queueError}
              </div>
            )}
            {queueSnapshot && (
              <div style={{ display: 'grid', gap: 12, marginBottom: 12 }}>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                  {[
                    { label: 'Scans', count: queueSnapshot.queues.scan.length },
                    { label: 'Post-scan', count: queueSnapshot.queues.postScan.length },
                    { label: 'Mix', count: queueSnapshot.queues.mix.length },
                    { label: 'Deep analysis', count: queueSnapshot.queues.deepAnalysis.length },
                  ].map((item) => (
                    <div
                      key={item.label}
                      style={{
                        padding: '6px 10px',
                        borderRadius: 999,
                        border: '1px solid var(--border)',
                        background: 'var(--bg)',
                        color: 'var(--text-muted)',
                        fontSize: 11,
                      }}
                    >
                      <strong style={{ color: 'var(--text)', fontWeight: 600 }}>{item.count}</strong> {item.label}
                    </div>
                  ))}
                </div>
                <div style={{ display: 'grid', gap: 10 }}>
                  <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>Live queue</div>
                  {(queueSnapshot.queues.scan.length > 0 || queueSnapshot.queues.postScan.length > 0) ? (
                  <div style={{ display: 'grid', gap: 10 }}>
                    {queueSnapshot.queues.scan.map((entry) => {
                      const busyKey = `scan:${entry.id}`;
                      const actionLabel = entry.status === 'running' ? 'Stop scan' : 'Cancel scan';
                      return (
                        <div key={`scan-${entry.id}`} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, padding: '10px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--bg)' }}>
                          <div style={{ minWidth: 0 }}>
                            <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
                              <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>{entry.library_name || 'Library'} scan</div>
                              <span style={{ fontSize: 10, color: 'var(--accent)', border: '1px solid var(--border)', borderRadius: 999, padding: '2px 7px' }}>
                                {formatQueueStateLabel(entry.status)}
                              </span>
                            </div>
                            <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 4 }}>
                              {entry.files_found != null || entry.files_scanned != null
                                ? `${entry.files_scanned ?? 0} of ${entry.files_found ?? 0} files scanned`
                                : 'Preparing scan'}
                              {entry.started_at ? ` • Started ${fmtQueueTime(entry.started_at)}` : ''}
                              {entry.errors != null ? ` • Errors ${entry.errors}` : ''}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() => void runQueueAction(busyKey, `Scan job ${entry.id} cancelled.`, () => api.admin.cancelScanJob(entry.id))}
                            disabled={queueActionBusy != null}
                            style={{ ...P.btnSecondary, minWidth: 108, textAlign: 'center' }}
                          >
                            {queueActionBusy === busyKey ? 'Working…' : actionLabel}
                          </button>
                        </div>
                      );
                    })}
                    {queueSnapshot.queues.postScan.map((entry) => {
                      const isRunning = entry.status === 'running';
                      const busyKey = `post-scan:${entry.id}`;
                      const actionLabel = isRunning ? 'Stop task' : 'Cancel task';
                      const action = isRunning
                        ? () => api.admin.failPostScanJob(entry.id)
                        : () => api.admin.cancelPostScanJob(entry.id);
                      const successMessage = isRunning
                        ? `Post-scan job ${entry.id} stopped.`
                        : `Post-scan job ${entry.id} cancelled.`;
                      return (
                        <div key={`post-scan-${entry.id}`} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, padding: '10px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--bg)' }}>
                          <div style={{ minWidth: 0 }}>
                            <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
                              <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>
                                {entry.library_name || 'Library'} {entry.job_type ? `• ${entry.job_type}` : 'post-scan'}
                              </div>
                              <span style={{ fontSize: 10, color: 'var(--accent)', border: '1px solid var(--border)', borderRadius: 999, padding: '2px 7px' }}>
                                {formatQueueStateLabel(entry.status)}
                              </span>
                            </div>
                            <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 4 }}>
                              {entry.current_step || 'Waiting for worker'}
                              {entry.started_at ? ` • Started ${fmtQueueTime(entry.started_at)}` : ''}
                              {entry.error_message ? ` • ${entry.error_message}` : ''}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() => void runQueueAction(busyKey, successMessage, action)}
                            disabled={queueActionBusy != null}
                            style={{ ...P.btnSecondary, minWidth: 108, textAlign: 'center' }}
                          >
                            {queueActionBusy === busyKey ? 'Working…' : actionLabel}
                          </button>
                        </div>
                      );
                    })}
                  </div>
                  ) : (
                    <div style={{ padding: '10px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--bg)', fontSize: 11, color: 'var(--text-muted)' }}>
                      No active scan or post-scan jobs right now.
                    </div>
                  )}
                </div>
                {libraries.length > 0 && (
                  <div style={{ display: 'grid', gap: 10 }}>
                    <div>
                      <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>Run maintenance</div>
                      <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 4 }}>
                        Queue a follow-up job for a specific library. These run in the background and show up in the live queue above.
                      </div>
                    </div>
                    {libraries.map((library) => (
                      <div key={`manual-post-scan-${library.id}`} style={{ padding: '10px 12px', borderRadius: 8, border: '1px solid var(--border)', background: 'var(--bg)' }}>
                        <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
                          <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>{library.name}</div>
                          <span style={{ fontSize: 10, color: 'var(--text-muted)', border: '1px solid var(--border)', borderRadius: 999, padding: '2px 7px' }}>
                            {'MUSIC'}
                          </span>
                        </div>
                        <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 4, marginBottom: 8 }}>
                          Choose a background maintenance task for this library.
                        </div>
                        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                          {getLibraryPostScanActions(library).map((action) => {
                            const busyKey = `enqueue:${library.id}:${action.jobType}`;
                            return (
                              <button
                                key={`${library.id}-${action.jobType}`}
                                type="button"
                                onClick={() => void runQueueAction(
                                  busyKey,
                                  `${action.label} queued for ${library.name}.`,
                                  () => api.admin.enqueuePostScanJob(library.id, action.jobType),
                                )}
                                disabled={queueActionBusy != null}
                                style={P.btnSecondary}
                              >
                                {queueActionBusy === busyKey ? 'Queuing…' : action.label}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
            <div style={{ borderTop: '1px solid var(--border)', paddingTop: 12 }}>
              <button
                type="button"
                onClick={() => setShowRawQueueSnapshot((value) => !value)}
                style={{ ...P.btnSecondary, padding: '7px 12px', fontSize: 11 }}
              >
                {showRawQueueSnapshot ? 'Hide raw queue snapshot' : 'Show raw queue snapshot'}
              </button>
              {showRawQueueSnapshot && (
                <textarea
                  aria-label="Queue Snapshot"
                  readOnly
                  value={formatQueueSnapshot(queueSnapshot)}
                  style={{
                    width: '100%',
                    minHeight: 220,
                    resize: 'vertical',
                    boxSizing: 'border-box',
                    borderRadius: 8,
                    border: '1px solid var(--border)',
                    background: 'var(--bg)',
                    color: 'var(--text)',
                    padding: '12px 14px',
                    fontSize: 11,
                    lineHeight: 1.6,
                    fontFamily: 'Consolas, Monaco, monospace',
                    marginTop: 10,
                  }}
                />
              )}
            </div>
          </div>
        </div>
      )}

      {/* ── Advanced Tab ──────────────────────────────────────────────────── */}
      {activeTab === 'advanced' && (
        <div style={P.section}>
          <div style={P.sectionTitle}>Playback</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 10 }}>
              Vinyl Mode
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 12, cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={playbackMode === 'vinyl'}
                  onChange={(e) => onPlaybackModeChange?.(e.target.checked ? 'vinyl' : 'standard')}
                />
                Enable Vinyl Mode
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 12, cursor: 'pointer', opacity: playbackMode === 'vinyl' ? 1 : 0.6 }}>
                <input
                  type="checkbox"
                  checked={vinylHardcore}
                  disabled={playbackMode !== 'vinyl'}
                  onChange={(e) => onVinylHardcoreChange?.(e.target.checked)}
                />
                Hardcore Vinyl (no seeking / no jumping)
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 12, cursor: 'pointer', opacity: playbackMode === 'vinyl' ? 1 : 0.6 }}>
                <input
                  type="checkbox"
                  checked={vinylNeedleDrop}
                  disabled={playbackMode !== 'vinyl'}
                  onChange={(e) => onVinylNeedleDropChange?.(e.target.checked)}
                />
                Needle-drop sound
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 12, cursor: 'pointer', opacity: playbackMode === 'vinyl' ? 1 : 0.6 }}>
                <input
                  type="checkbox"
                  checked={vinylAnalogFxDisabled}
                  disabled={playbackMode !== 'vinyl'}
                  onChange={(e) => onVinylAnalogFxDisabledChange?.(e.target.checked)}
                />
                Disable analog noise effects
              </label>
              <div style={{ opacity: playbackMode === 'vinyl' && !vinylAnalogFxDisabled ? 1 : 0.5 }}>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>
                  Needle-drop intensity: {Math.round(vinylNeedleDropIntensity * 100)}%
                </div>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={1}
                  value={Math.round(vinylNeedleDropIntensity * 100)}
                  disabled={playbackMode !== 'vinyl' || vinylAnalogFxDisabled}
                  onChange={(e) => onVinylNeedleDropIntensityChange?.(Math.max(0, Math.min(1, Number(e.target.value) / 100)))}
                  style={{ width: 260 }}
                />
              </div>
            </div>
          </div>

          <div style={{
            display: 'flex', alignItems: 'flex-start', gap: 20,
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                Server-side transcoding
              </div>
              <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                When enabled, the server converts FLAC, M4A, AAC, WMA and other
                formats to MP3 before streaming. Disable to stream the raw file
                bytes directly — useful when your browser natively supports the
                format (e.g. Safari with FLAC/AAC) or when transcoding causes issues.
              </div>
            </div>
            {/* Toggle */}
            <div
              onClick={() => {
                const next = !streamDirect;
                setStreamDirect(next);
                setStreamDirectState(next);
                onStreamDirectChange?.(next);
              }}
              title={streamDirect ? 'Transcoding disabled — click to enable' : 'Transcoding enabled — click to disable'}
              style={{
                width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                position: 'relative', flexShrink: 0, marginTop: 2,
                backgroundColor: streamDirect ? 'var(--border)' : 'var(--accent)',
                transition: 'background 0.2s',
              }}
            >
              <div style={{
                position: 'absolute', top: 3,
                left: streamDirect ? 3 : 23,
                width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
              }} />
            </div>
          </div>

          <div style={{ fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.6, padding: '0 4px' }}>
            <strong style={{ color: 'var(--text)' }}>Note:</strong> This setting is stored as a browser
            cookie and applies only to this device/browser. Other browsers or devices accessing the
            same server will use their own setting. The change takes effect on the next track played.
          </div>
          <div style={{
            padding: '16px 20px', borderRadius: 8, marginTop: 12, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              Server transcoding quality
            </div>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
              Select MP3 output quality used when server-side transcoding is enabled.
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <select
                value={transcodeQuality}
                onChange={e => { setTranscodeQuality(e.target.value as 'low' | 'high'); setTranscodeQualityResult(null); }}
                style={{
                  background: 'var(--bg)', border: '1px solid var(--border)', color: 'var(--text)',
                  borderRadius: 6, padding: '7px 10px', fontSize: 12, fontFamily: 'inherit', outline: 'none',
                }}
              >
                <option value="low">Low (192 kbps CBR)</option>
                <option value="high">High (320 kbps CBR)</option>
              </select>
              <button
                disabled={transcodeQualitySaving}
                onClick={async () => {
                  setTranscodeQualitySaving(true);
                  setTranscodeQualityResult(null);
                  try {
                    await api.settings.update({ transcodeQuality });
                    setTranscodeQualityResult('Saved');
                  } catch (e: any) {
                    setTranscodeQualityResult(`Error: ${e.message}`);
                  } finally {
                    setTranscodeQualitySaving(false);
                  }
                }}
                style={{
                  padding: '7px 20px', borderRadius: 6, border: '1px solid var(--accent)',
                  background: 'var(--accent)', color: '#fff', fontSize: 12, cursor: 'pointer',
                  opacity: transcodeQualitySaving ? 0.6 : 1,
                }}
              >
                {transcodeQualitySaving ? 'Saving...' : 'Save Quality'}
              </button>
              {transcodeQualityResult && (
                <span style={{ fontSize: 12, color: transcodeQualityResult.startsWith('Error') ? '#ef4444' : '#22c55e' }}>
                  {transcodeQualityResult}
                </span>
              )}
            </div>
            {/* ReplayGain normalization toggle */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginTop: 14 }}>
              <div
                onClick={async () => {
                  const next = !replayGainEnabled;
                  setReplayGainEnabled(next);
                  await api.settings.update({ replayGainEnabled: String(next) });
                }}
                title={replayGainEnabled ? 'ReplayGain normalization on — click to disable' : 'ReplayGain normalization off — click to enable'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0,
                  backgroundColor: replayGainEnabled ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: replayGainEnabled ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
              <div>
                <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text)' }}>ReplayGain normalization</div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 1 }}>
                  Normalize loudness across all tracks using EBU R128. Applies during server-side transcoding only.
                </div>
              </div>
            </div>
            {/* Default Vinyl Mode */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginTop: 14 }}>
              <div
                onClick={async () => {
                  const next = !defaultVinylMode;
                  setDefaultVinylMode(next);
                  await api.settings.update({ vinylMode: next ? 'vinyl' : 'standard' });
                }}
                title={defaultVinylMode ? 'Default vinyl mode on — click to disable' : 'Default vinyl mode off — click to enable'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0,
                  backgroundColor: defaultVinylMode ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: defaultVinylMode ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
              <div>
                <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--text)' }}>Default vinyl mode</div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 1 }}>
                  When enabled, all users start in vinyl mode on login.
                </div>
              </div>
            </div>
          </div>

          {/* ── Track Transitions ──────────────────────────────────────── */}
          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              BoogieMix output folder
            </div>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
              Admin override for rendered mix files. Leave blank to use the default folder inside the active database directory (`mix-outputs`).
            </div>
            <div style={{ fontSize: 12, color: '#f59e0b', lineHeight: 1.6, marginBottom: 10, fontWeight: 600 }}>
              BoogieMix is experimental and output quality may vary between runs.
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <input
                type="text"
                value={boogiemixOutputFolder}
                onChange={(e) => { setBoogiemixOutputFolder(e.target.value); setBoogiemixOutputFolderResult(null); }}
                placeholder="Blank = <db-folder>\\mix-outputs"
                style={{
                  minWidth: 320,
                  flex: 1,
                  background: 'var(--bg)',
                  border: '1px solid var(--border)',
                  color: 'var(--text)',
                  borderRadius: 6,
                  padding: '7px 10px',
                  fontSize: 12,
                  fontFamily: 'inherit',
                  outline: 'none',
                }}
              />
              <button
                disabled={boogiemixOutputFolderSaving}
                onClick={async () => {
                  setBoogiemixOutputFolderSaving(true);
                  setBoogiemixOutputFolderResult(null);
                  try {
                    await api.settings.update({ boogiemixOutputFolder });
                    setBoogiemixOutputFolderResult('Saved');
                  } catch (e: any) {
                    setBoogiemixOutputFolderResult(`Error: ${e.message}`);
                  } finally {
                    setBoogiemixOutputFolderSaving(false);
                  }
                }}
                style={{
                  padding: '7px 20px', borderRadius: 6, border: '1px solid var(--accent)',
                  background: 'var(--accent)', color: '#fff', fontSize: 12, cursor: 'pointer',
                  opacity: boogiemixOutputFolderSaving ? 0.6 : 1,
                }}
              >
                {boogiemixOutputFolderSaving ? 'Saving...' : 'Save Folder'}
              </button>
              {boogiemixOutputFolderResult && (
                <span style={{ fontSize: 12, color: boogiemixOutputFolderResult.startsWith('Error') ? '#ef4444' : '#22c55e' }}>
                  {boogiemixOutputFolderResult}
                </span>
              )}
            </div>
          </div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              BoogieMix deep analysis
            </div>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
              Torch/Demucs analysis is separate from BPM analysis and is only used by High Quality BoogieMix jobs when the runtime is ready.
            </div>
            {boogiemixDeepLoading ? (
              <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>Loading deep analysis status...</div>
            ) : boogiemixDeepStatus ? (
              <div style={{ display: 'grid', gap: 10, fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.7 }}>
                <div style={{ color: boogiemixDeepStatus.runtime?.enabled ? '#22c55e' : '#f59e0b', fontWeight: 600 }}>
                  {boogiemixDeepStatus.runtime?.summary ?? 'Deep analysis runtime status unavailable.'}
                </div>
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 8 }}>
                  {(['python', 'torch', 'demucs', 'ffmpeg', 'gpu'] as const).map((name) => {
                    const component = boogiemixDeepStatus.runtime?.[name];
                    return (
                      <div key={name} style={{ border: '1px solid var(--border)', borderRadius: 6, padding: '8px 10px', background: 'var(--bg)' }}>
                        <div style={{ color: 'var(--text)', fontWeight: 600, textTransform: 'uppercase' }}>{name}</div>
                        <div>{component?.available ? 'Available' : 'Missing'}</div>
                        {component?.version && <div>{component.version}</div>}
                        {component?.detail && <div>{component.detail}</div>}
                      </div>
                    );
                  })}
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  {boogiemixDeepStatus.queue.running > 0 && (
                    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, background: 'rgba(34,197,94,0.15)', border: '1px solid #22c55e', borderRadius: 10, padding: '1px 8px', color: '#22c55e', fontWeight: 700, fontSize: 11 }}>
                      <span style={{ width: 7, height: 7, borderRadius: '50%', background: '#22c55e', display: 'inline-block', animation: 'bm-pulse 1.2s ease-in-out infinite' }} />
                      ANALYZING
                    </span>
                  )}
                  <span>Queue: {boogiemixDeepStatus.queue.pending} pending / {boogiemixDeepStatus.queue.running} running / {boogiemixDeepStatus.queue.failed} failed / {boogiemixDeepStatus.queue.skipped} skipped / {boogiemixDeepStatus.queue.done} done</span>
                </div>
                <div>
                  Cache: {boogiemixDeepStatus.cache.analyzedTracks} tracks analyzed, about {formatBytes(boogiemixDeepStatus.cache.estimatedBytes)} stored in SQLite
                  {boogiemixDeepStatus.cache.newestCreatedAt ? ` · newest ${fmtLastRun(boogiemixDeepStatus.cache.newestCreatedAt)}` : ''}
                </div>
                <div style={{ display: 'grid', gap: 8, marginTop: 2 }}>
                  <label style={{ display: 'grid', gap: 4 }}>
                    <span style={{ color: 'var(--text)', fontWeight: 600 }}>Background mode</span>
                    <select
                      value={boogiemixDeepBackgroundMode}
                      onChange={(e) => {
                        const value = e.target.value;
                        setBoogiemixDeepBackgroundMode(value);
                        runBoogieMixDeepAction('background-mode', async () => {
                          await api.settings.update({ boogiemixDeepAnalysisBackgroundMode: value });
                          return 'Background mode saved';
                        });
                      }}
                      style={{ maxWidth: 360, background: 'var(--bg)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 6, padding: '7px 10px', fontSize: 12 }}
                    >
                      <option value="off">Off</option>
                      <option value="playlists_only">Playlists only</option>
                      <option value="favorites_and_playlists">Favorites and playlists</option>
                      <option value="all_music">All music</option>
                    </select>
                  </label>
                  {boogiemixDeepBackgroundMode === 'all_music' && (
                    <div style={{ color: '#f59e0b', fontWeight: 600 }}>
                      Full-library deep analysis can take many hours and cause sustained CPU and disk activity.
                    </div>
                  )}
                  <label style={{ display: 'grid', gap: 4 }}>
                    <span style={{ color: 'var(--text)', fontWeight: 600 }}>Max track length</span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <input
                        type="number"
                        min={0}
                        max={480}
                        value={boogiemixDeepMaxDurationMins}
                        onChange={(e) => setBoogiemixDeepMaxDurationMins(Math.max(0, Number(e.target.value) || 0))}
                        onBlur={() => {
                          const value = String(boogiemixDeepMaxDurationMins);
                          runBoogieMixDeepAction('max-duration', async () => {
                            await api.settings.update({ boogiemixDeepAnalysisMaxDurationMins: value });
                            return 'Max track length saved';
                          });
                        }}
                        style={{ width: 80, background: 'var(--bg)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 6, padding: '7px 10px', fontSize: 12 }}
                      />
                      <span style={{ color: 'var(--text-muted)', fontSize: 12 }}>
                        minutes — tracks longer than this are skipped (0 = no limit). Timeout scales automatically at 5 s per track-second.
                      </span>
                    </div>
                  </label>
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
                    <select
                      value={boogiemixDeepSelectedLibrary}
                      onChange={(e) => setBoogiemixDeepSelectedLibrary(e.target.value as ClientEntityId)}
                      aria-label="BoogieMix deep-analysis library"
                      style={{ minWidth: 220, background: 'var(--bg)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 6, padding: '7px 10px', fontSize: 12 }}
                    >
                      <option value="">Select library</option>
                      {libraries.map((library) => (
                        <option key={String(library.id)} value={String(library.id)}>{library.name}</option>
                      ))}
                    </select>
                    <button
                      disabled={!boogiemixDeepSelectedLibrary || boogiemixDeepActionBusy === 'queue-library'}
                      onClick={() => runBoogieMixDeepAction('queue-library', async () => {
                        const result = await api.boogiemix.queueLibraryDeepAnalysis(boogiemixDeepSelectedLibrary);
                        return `Queued ${result.queued} tracks`;
                      })}
                      style={{ padding: '7px 12px', borderRadius: 6, border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', cursor: boogiemixDeepSelectedLibrary ? 'pointer' : 'not-allowed', opacity: !boogiemixDeepSelectedLibrary ? 0.55 : 1 }}
                    >
                      Analyze Library
                    </button>
                    <button
                      onClick={() => runBoogieMixDeepAction('pause', async () => {
                        if (boogiemixDeepPauseBackground) {
                          await api.boogiemix.resumeDeepAnalysisBackground();
                          setBoogiemixDeepPauseBackground(false);
                          return 'Background analysis resumed';
                        }
                        await api.boogiemix.pauseDeepAnalysisBackground();
                        setBoogiemixDeepPauseBackground(true);
                        return 'Background analysis paused';
                      })}
                      disabled={boogiemixDeepActionBusy === 'pause'}
                      style={{ padding: '7px 12px', borderRadius: 6, border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', cursor: 'pointer' }}
                    >
                      {boogiemixDeepPauseBackground ? 'Resume Background' : 'Pause Background'}
                    </button>
                    <button
                      onClick={() => {
                        if (!window.confirm('Clear stored BoogieMix deep-analysis cache?')) return;
                        runBoogieMixDeepAction('clear-cache', async () => {
                          const result = await api.boogiemix.clearDeepAnalysisCache();
                          return `Cleared ${result.deletedCacheRows} cached rows`;
                        });
                      }}
                      disabled={boogiemixDeepActionBusy === 'clear-cache'}
                      style={{ padding: '7px 12px', borderRadius: 6, border: '1px solid #ef4444', background: 'transparent', color: '#ef4444', cursor: 'pointer' }}
                    >
                      Clear Cache
                    </button>
                  </div>
                  {boogiemixDeepActionResult && (
                    <div style={{ color: boogiemixDeepActionResult.startsWith('Error') ? '#ef4444' : '#22c55e', fontWeight: 600 }}>
                      {boogiemixDeepActionResult}
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <div style={{ fontSize: 11, color: '#f59e0b' }}>
                Deep analysis status unavailable. High Quality mixes will report fallback details when created.
              </div>
            )}
          </div>

          <div style={{ ...P.sectionTitle, marginTop: 28 }}>Track Transitions</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
              Control how tracks transition during playback. Override per-album or per-playlist via their context menus.
            </div>

            {/* Mode picker — 3 pill buttons */}
            <div style={{ marginBottom: 16 }}>
              <label style={{ fontSize: 12, color: 'var(--text-muted)', display: 'block', marginBottom: 6 }}>
                Transition mode
              </label>
              <div style={{ display: 'flex', gap: 0, borderRadius: 8, overflow: 'hidden', border: '1px solid var(--border)', width: 'fit-content' }}>
                {([
                  { value: 'off', label: 'Off' },
                  { value: 'zerogap', label: 'Zero-gap' },
                  { value: 'crossfade', label: 'Crossfade' },
                ] as const).map(opt => (
                  <button
                    key={opt.value}
                    onClick={async () => {
                      setCfMode(opt.value);
                      setCfSaving(true);
                      setCfResult(null);
                      try {
                        await api.settings.update({ crossfadeMode: opt.value });
                        setCfResult('Saved');
                      } catch { setCfResult('Error'); }
                      setCfSaving(false);
                      setTimeout(() => setCfResult(null), 2000);
                    }}
                    style={{
                      padding: '7px 16px', fontSize: 12, fontWeight: 600, cursor: 'pointer',
                      border: 'none', outline: 'none',
                      backgroundColor: cfMode === opt.value ? 'var(--accent)' : 'var(--bg)',
                      color: cfMode === opt.value ? '#fff' : 'var(--text)',
                      transition: 'background 0.15s, color 0.15s',
                    }}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Duration slider — visible only when crossfade mode */}
            {cfMode === 'crossfade' && (
              <div style={{ marginBottom: 8 }}>
                <label style={{ fontSize: 12, color: 'var(--text-muted)', display: 'block', marginBottom: 6 }}>
                  Crossfade duration: <strong style={{ color: 'var(--text)' }}>{cfDuration}s</strong>
                </label>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>1s</span>
                  <input
                    type="range"
                    min={1}
                    max={10}
                    step={1}
                    value={cfDuration}
                    onChange={async e => {
                      const val = Number(e.target.value);
                      setCfDuration(val);
                      setCfSaving(true);
                      setCfResult(null);
                      try {
                        await api.settings.update({ crossfadeDuration: String(val) });
                        setCfResult('Saved');
                      } catch { setCfResult('Error'); }
                      setCfSaving(false);
                      setTimeout(() => setCfResult(null), 2000);
                    }}
                    style={{ flex: 1, accentColor: 'var(--accent)' }}
                  />
                  <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>10s</span>
                </div>
              </div>
            )}

            {/* Save status */}
            {(cfSaving || cfResult) && (
              <div style={{ fontSize: 11, color: cfResult === 'Error' ? '#e74c3c' : 'var(--text-muted)', marginTop: 4 }}>
                {cfSaving ? 'Saving…' : cfResult}
              </div>
            )}
          </div>

          {/* ── Waveforms ───────────────────────────────────────────────── */}
          <div style={{ ...P.sectionTitle, marginTop: 28 }}>Waveforms</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 12 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Scan debug logging
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  Writes detailed scan and post-scan backend diagnostics to <code style={{ color: 'var(--accent)' }}>logs/debug.log</code>.
                  Leave this disabled unless you are troubleshooting large-library scans or stuck post-scan work.
                </div>
              </div>
              <div
                onClick={async () => {
                  if (scanDebugSaving) return;
                  const next = !scanDebugLoggingEnabled;
                  setScanDebugLoggingEnabled(next);
                  await saveScanDebugSettings(next);
                }}
                title={scanDebugLoggingEnabled ? 'On' : 'Off'}
                style={{
                  width: 44, height: 24, borderRadius: 12,
                  cursor: scanDebugSaving ? 'default' : 'pointer',
                  position: 'relative', flexShrink: 0, marginTop: 2,
                  opacity: scanDebugSaving ? 0.6 : 1,
                  backgroundColor: scanDebugLoggingEnabled ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: scanDebugLoggingEnabled ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
            </div>

            <div style={{ fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.7 }}>
              Debug mode is <strong style={{ color: 'var(--text)' }}>{scanDebugLoggingEnabled ? 'enabled' : 'disabled'}</strong>.
              When enabled, scan queueing, file and batch checkpoints, follow-up job dispatch, worker progress, failures, and completion events are appended to the shared debug log.
            </div>

            {(scanDebugSaving || scanDebugResult) && (
              <div style={{ fontSize: 11, marginTop: 10, color: scanDebugResult?.startsWith('Error') ? '#ef4444' : 'var(--text-muted)' }}>
                {scanDebugSaving ? 'Saving debug logging setting...' : scanDebugResult}
              </div>
            )}
          </div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 16 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Generate waveform when missing
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  If enabled, BoogieBox starts waveform generation on playback for tracks that do not already
                  have cached waveform data.
                </div>
              </div>
              <div
                onClick={async () => {
                  const next = !waveformGenerateOnMissing;
                  setWaveformGenerateOnMissing(next);
                  await saveWaveformSettings({ waveformGenerateOnMissing: next ? 'true' : 'false' });
                }}
                title={waveformGenerateOnMissing ? 'On' : 'Off'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0, marginTop: 2,
                  backgroundColor: waveformGenerateOnMissing ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: waveformGenerateOnMissing ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
            </div>

            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 16 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Enable waveform background mapping
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  Runs a scheduled background task that maps waveform data for tracks that are still missing it.
                </div>
              </div>
              <div
                onClick={async () => {
                  const next = !waveformBackgroundEnabled;
                  setWaveformBackgroundEnabled(next);
                  await saveWaveformSettings({ waveformBackgroundEnabled: next ? 'true' : 'false' });
                }}
                title={waveformBackgroundEnabled ? 'On' : 'Off'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0, marginTop: 2,
                  backgroundColor: waveformBackgroundEnabled ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: waveformBackgroundEnabled ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
            </div>

            <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap', marginBottom: 16 }}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <label style={{ fontSize: 11, color: 'var(--text-muted)' }}>Mapping frequency</label>
                <select
                  value={waveformFrequencyHours}
                  onChange={async (e) => {
                    const next = Number(e.target.value);
                    setWaveformFrequencyHours(next);
                    await saveWaveformSettings({ waveformBackgroundFrequencyHours: String(next) });
                  }}
                  disabled={!waveformBackgroundEnabled}
                  style={{
                    background: 'var(--bg)', border: '1px solid var(--border)', color: 'var(--text)',
                    borderRadius: 6, padding: '6px 10px', fontSize: 12, fontFamily: 'inherit',
                    opacity: waveformBackgroundEnabled ? 1 : 0.5,
                  }}
                >
                  {FREQ_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </select>
              </div>

              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <label style={{ fontSize: 11, color: 'var(--text-muted)' }}>Batch size per run</label>
                <select
                  value={waveformBatchSize}
                  onChange={async (e) => {
                    const next = Number(e.target.value);
                    setWaveformBatchSize(next);
                    await saveWaveformSettings({ waveformBackgroundBatchSize: String(next) });
                  }}
                  style={{
                    background: 'var(--bg)', border: '1px solid var(--border)', color: 'var(--text)',
                    borderRadius: 6, padding: '6px 10px', fontSize: 12, fontFamily: 'inherit',
                  }}
                >
                  {WAVEFORM_BATCH_OPTIONS.map((opt) => (
                    <option key={opt} value={opt}>{opt} tracks</option>
                  ))}
                </select>
              </div>

              <button
                onClick={runWaveformMappingNow}
                style={{
                  marginTop: 18,
                  padding: '7px 14px',
                  borderRadius: 6,
                  border: '1px solid var(--border)',
                  background: 'var(--bg)',
                  color: 'var(--text)',
                  fontSize: 12,
                  cursor: 'pointer',
                }}
              >
                Run Mapping Now
              </button>
            </div>

            <div style={{ fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.7 }}>
              {waveformLoading ? 'Loading waveform status...' : (
                waveformStatus
                  ? <>
                      <div>Coverage: {waveformStatus.mappedTracks} mapped / {waveformStatus.totalTracks} total ({waveformStatus.missingTracks} missing)</div>
                      <div>Last run: {fmtLastRun(waveformStatus.lastRun)}</div>
                      <div>Next run: {waveformStatus.enabled ? fmtNextRun(waveformStatus.nextRun) : 'Disabled'}</div>
                      {waveformStatus.inProgress && waveformStatus.activeRun && (
                        <div>
                          In progress: {waveformStatus.activeRun.processed}/{waveformStatus.activeRun.totalMissing} processed
                        </div>
                      )}
                    </>
                  : <div>Waveform mapping status unavailable.</div>
              )}
            </div>

            {(waveformSaving || waveformSettingsResult || waveformRunResult) && (
              <div style={{ fontSize: 11, marginTop: 10, color: 'var(--text-muted)' }}>
                {waveformSaving ? 'Saving waveform settings...' : waveformSettingsResult}
                {waveformRunResult ? <div style={{ marginTop: 4 }}>{waveformRunResult}</div> : null}
              </div>
            )}
          </div>

          {/* ── BPM Analysis ────────────────────────────────────────────── */}
          <div style={{ ...P.sectionTitle, marginTop: 28 }}>BPM Analysis</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 16 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Enable BPM background analysis
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  Runs BPM analysis on a schedule for tracks that are still missing BPM data.
                </div>
              </div>
              <div
                onClick={async () => {
                  const next = !bpmBackgroundEnabled;
                  setBpmBackgroundEnabled(next);
                  await saveBpmSettings({ bpmBackgroundEnabled: next ? 'true' : 'false' });
                }}
                title={bpmBackgroundEnabled ? 'On' : 'Off'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0, marginTop: 2,
                  backgroundColor: bpmBackgroundEnabled ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: bpmBackgroundEnabled ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
            </div>

            <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap', marginBottom: 12 }}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <label style={{ fontSize: 11, color: 'var(--text-muted)' }}>Analysis frequency</label>
                <select
                  value={bpmFrequencyHours}
                  onChange={async (e) => {
                    const next = Number(e.target.value);
                    setBpmFrequencyHours(next);
                    await saveBpmSettings({ bpmBackgroundFrequencyHours: String(next) });
                  }}
                  disabled={!bpmBackgroundEnabled}
                  style={{
                    background: 'var(--bg)', border: '1px solid var(--border)', color: 'var(--text)',
                    borderRadius: 6, padding: '6px 10px', fontSize: 12, fontFamily: 'inherit',
                    opacity: bpmBackgroundEnabled ? 1 : 0.5,
                  }}
                >
                  {FREQ_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </select>
              </div>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 12 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  BPM Detection
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  Detect BPM for tracks using local FFmpeg audio analysis.
                  Results are used by BoogieMix for better tempo matching and transitions.
                </div>
              </div>
              <button
                onClick={async () => {
                  setBpmRunResult('Running...');
                  try {
                    const result = await api.bpm.run();
                    setBpmRunResult(`Done: ${result.analyzed} analyzed, ${result.skipped} skipped, ${result.errors} errors`);
                    loadBpmStatus();
                  } catch (e: any) {
                    setBpmRunResult(`Error: ${e.message}`);
                  }
                }}
                style={{
                  padding: '8px 16px', borderRadius: 6, border: '1px solid var(--border)',
                  backgroundColor: 'var(--surface-hover)', color: 'var(--text)', fontSize: 12,
                  cursor: 'pointer', whiteSpace: 'nowrap',
                }}
              >
                Run BPM Analysis
              </button>
            </div>

            <div style={{ fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.7 }}>
              {bpmLoading ? 'Loading BPM status...' : (
                bpmStatus
                  ? <>
                      <div>Coverage: {bpmStatus.analyzedTracks} analyzed / {bpmStatus.totalTracks} total ({bpmStatus.missingTracks} pending)</div>
                      <div>Spotify fallback: {bpmStatus.spotifyFallbackEnabled ? 'Enabled' : 'Disabled'}</div>
                      <div>Next run: {bpmStatus.backgroundEnabled ? fmtNextRun(bpmStatus.nextRun) : 'Disabled'}</div>
                      <div>Last run: {fmtLastRun(bpmStatus.lastRun)}</div>
                      {bpmStatus.inProgress && bpmStatus.activeRun && (
                        <div>In progress: {bpmStatus.activeRun.processed} processed, {bpmStatus.activeRun.analyzed} analyzed</div>
                      )}
                    </>
                  : <div>BPM analysis status unavailable.</div>
              )}
            </div>

            {(bpmSaving || bpmSettingsResult || bpmRunResult) && (
              <div style={{ fontSize: 11, marginTop: 10, color: 'var(--text-muted)' }}>
                {bpmSaving ? 'Saving BPM settings...' : bpmSettingsResult}
                {bpmRunResult ? <div style={{ marginTop: 4 }}>{bpmRunResult}</div> : null}
              </div>
            )}
          </div>

          {/* ── DLNA Server ─────────────────────────────────────────────── */}
          <div style={{ ...P.sectionTitle, marginTop: 28 }}>DLNA Server</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            {/* Enable toggle */}
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 16 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Enable DLNA server
                </div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  Broadcast your music libraries on the local network so DLNA-compatible
                  devices can discover and stream audio.
                </div>
              </div>
              <div
                onClick={() => setDlnaEnabled(!dlnaEnabled)}
                title={dlnaEnabled ? 'DLNA enabled — click to disable' : 'DLNA disabled — click to enable'}
                style={{
                  width: 44, height: 24, borderRadius: 12, cursor: 'pointer',
                  position: 'relative', flexShrink: 0, marginTop: 2,
                  backgroundColor: dlnaEnabled ? 'var(--accent)' : 'var(--border)',
                  transition: 'background 0.2s',
                }}
              >
                <div style={{
                  position: 'absolute', top: 3,
                  left: dlnaEnabled ? 23 : 3,
                  width: 18, height: 18, borderRadius: '50%', backgroundColor: '#fff',
                  transition: 'left 0.2s', boxShadow: '0 1px 3px rgba(0,0,0,0.35)',
                }} />
              </div>
            </div>

            {/* Friendly name */}
            <div style={{ marginBottom: 12 }}>
              <label style={{ fontSize: 12, color: 'var(--text-muted)', display: 'block', marginBottom: 4 }}>
                Device name (shown on network)
              </label>
              <input
                type="text"
                value={dlnaFriendlyName}
                onChange={e => setDlnaFriendlyName(e.target.value)}
                placeholder="BoogieBox"
                style={{
                  width: '100%', maxWidth: 280, background: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '7px 10px', fontSize: 12,
                  fontFamily: 'monospace', outline: 'none', boxSizing: 'border-box',
                }}
              />
            </div>

            {/* Port */}
            <div style={{ marginBottom: 16 }}>
              <label style={{ fontSize: 12, color: 'var(--text-muted)', display: 'block', marginBottom: 4 }}>
                DLNA HTTP port
              </label>
              <input
                type="number"
                value={dlnaPort}
                onChange={e => setDlnaPort(e.target.value)}
                placeholder="8200"
                min={1024}
                max={65535}
                style={{
                  width: 120, background: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '7px 10px', fontSize: 12,
                  fontFamily: 'monospace', outline: 'none',
                }}
              />
            </div>

            {/* Status indicator */}
            {dlnaStatus && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
                <div style={{
                  width: 8, height: 8, borderRadius: '50%',
                  backgroundColor: dlnaStatus.running ? '#22c55e' : 'var(--border)',
                }} />
                <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                  {dlnaStatus.running
                    ? `Running on port ${dlnaStatus.port} as "${dlnaStatus.friendlyName}"`
                    : 'Stopped'}
                </span>
              </div>
            )}

            {/* Save button */}
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <button
                disabled={dlnaSaving}
                onClick={async () => {
                  setDlnaSaving(true);
                  setDlnaResult(null);
                  try {
                    if (!isValidDlnaPort(dlnaPort)) {
                      setDlnaResult('Error: Port must be an integer between 1024 and 65535');
                      return;
                    }
                    const parsedPort = Number(dlnaPort.trim());
                    await api.settings.update({
                      dlnaEnabled: dlnaEnabled ? 'true' : 'false',
                      dlnaFriendlyName: dlnaFriendlyName.trim() || 'BoogieBox',
                      dlnaPort: String(parsedPort),
                    });
                    setDlnaResult('Saved');
                    // Refresh status after a brief delay for the worker to restart
                    setTimeout(() => loadDlnaStatus(), 1500);
                  } catch (e: any) {
                    setDlnaResult(`Error: ${e.message}`);
                  } finally {
                    setDlnaSaving(false);
                  }
                }}
                style={{
                  padding: '7px 20px', borderRadius: 6, border: '1px solid var(--accent)',
                  background: 'var(--accent)', color: '#fff', fontSize: 12, cursor: 'pointer',
                  opacity: dlnaSaving ? 0.6 : 1,
                }}
              >
                {dlnaSaving ? 'Saving…' : 'Save DLNA Settings'}
              </button>
              {dlnaResult && (
                <span style={{ fontSize: 12, color: dlnaResult.startsWith('Error') ? '#ef4444' : '#22c55e' }}>
                  {dlnaResult}
                </span>
              )}
            </div>
          </div>

          {/* ── Database (admin only) ─────────────────────────────────────── */}
          {isAdmin && (
            <>
              <div style={{ ...P.sectionTitle, marginTop: 28 }}>Database</div>
              <div style={{
                padding: '16px 20px', borderRadius: 8, marginBottom: 12,
                backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
              }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Active database folder
                </div>
                {currentDbFolder && (
                  <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12, fontFamily: 'monospace', wordBreak: 'break-all' }}>
                    {currentDbFolder}
                  </div>
                )}
                <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
                  Switch to a different database folder. The server will reload all settings from the new database.
                  If the folder does not contain a database yet, a fresh one will be created.
                </div>
                <div style={{ fontSize: 12, color: '#f59e0b', lineHeight: 1.6, marginBottom: 10, fontWeight: 600 }}>
                  Warning: switching databases reloads the server state. You will be logged out and the page will reload.
                </div>
                <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
                  <input
                    type="text"
                    value={switchDbFolder}
                    onChange={e => { setSwitchDbFolder(e.target.value); setSwitchDbResult(null); }}
                    placeholder="Enter folder path..."
                    style={{
                      minWidth: 320, flex: 1,
                      background: 'var(--bg)', border: '1px solid var(--border)',
                      color: 'var(--text)', borderRadius: 6, padding: '7px 10px',
                      fontSize: 12, fontFamily: 'inherit', outline: 'none',
                    }}
                  />
                  <button
                    onClick={() => setShowDbFolderPicker(true)}
                    style={{
                      padding: '7px 14px', borderRadius: 6, border: '1px solid var(--border)',
                      background: 'var(--bg)', color: 'var(--text)', fontSize: 12, cursor: 'pointer',
                    }}
                  >
                    Browse...
                  </button>
                  <button
                    disabled={switchDbSaving || !switchDbFolder.trim()}
                    onClick={async () => {
                      if (!window.confirm(`Switch to database at:\n\n${switchDbFolder.trim()}\n\nThe page will reload after switching.`)) return;
                      setSwitchDbSaving(true);
                      setSwitchDbResult(null);
                      try {
                        await api.systemSwitchDb(switchDbFolder.trim());
                        setSwitchDbResult('Switched — reloading...');
                        setTimeout(() => window.location.reload(), 800);
                      } catch (e: any) {
                        setSwitchDbResult(`Error: ${e.message}`);
                        setSwitchDbSaving(false);
                      }
                    }}
                    style={{
                      padding: '7px 20px', borderRadius: 6, border: '1px solid var(--accent)',
                      background: 'var(--accent)', color: '#fff', fontSize: 12, cursor: 'pointer',
                      opacity: (switchDbSaving || !switchDbFolder.trim()) ? 0.6 : 1,
                    }}
                  >
                    {switchDbSaving ? 'Switching...' : 'Switch Database'}
                  </button>
                </div>
                {switchDbResult && (
                  <div style={{ fontSize: 12, marginTop: 8, color: switchDbResult.startsWith('Error') ? '#ef4444' : '#22c55e' }}>
                    {switchDbResult}
                  </div>
                )}
              </div>
              {showDbFolderPicker && (
                <FolderPickerModal
                  initialPath={switchDbFolder || currentDbFolder || undefined}
                  onSelect={path => { setSwitchDbFolder(path); setShowDbFolderPicker(false); }}
                  onClose={() => setShowDbFolderPicker(false)}
                />
              )}
            </>
          )}
        </div>
      )}

      {/* ── Integrations Tab ──────────────────────────────────────────────── */}
      {activeTab === 'integrations' && (
        <div style={P.section}>

          {/* Discogs */}
          <div style={P.sectionTitle}>Discogs</div>
          <p style={{ color: 'var(--text-muted)', fontSize: 12, marginBottom: 20, lineHeight: 1.7 }}>
            Discogs is used to fetch album cover art when no local <code style={{ color: 'var(--accent)' }}>folder.jpg</code> is found.
            A free personal access token is required.
          </p>

          <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
              To get a free token:
              &nbsp;<strong style={{ color: 'var(--text)' }}>1.</strong> Sign in at{' '}
              <a href="https://www.discogs.com/settings/developers" target="_blank" rel="noreferrer"
                style={{ color: 'var(--accent)' }}>discogs.com/settings/developers</a>
              &nbsp;<strong style={{ color: 'var(--text)' }}>2.</strong> Click <em>Generate new token</em>
              &nbsp;<strong style={{ color: 'var(--text)' }}>3.</strong> Paste it below
            </div>

            <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', flexWrap: 'wrap' }}>
              <input
                type="password"
                placeholder="Paste your Discogs personal access token…"
                value={discogsToken}
                onChange={e => { setDiscogsToken(e.target.value); setDiscogsTestResult(null); }}
                style={{
                  flex: 1, minWidth: 240,
                  backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                  fontSize: 13, fontFamily: 'monospace', outline: 'none',
                }}
              />
              <button onClick={testDiscogsToken} disabled={discogsSaving} style={{ ...P.btnSecondary, whiteSpace: 'nowrap' }}>
                Test
              </button>
              <button onClick={saveDiscogsToken} disabled={discogsSaving} style={{ ...P.btnPrimary, whiteSpace: 'nowrap' }}>
                {discogsSaving ? 'Saving…' : 'Save'}
              </button>
            </div>

            {discogsTestResult && (
              <div style={{
                marginTop: 10, padding: '8px 12px', borderRadius: 6, fontSize: 12,
                backgroundColor: discogsTestResult.startsWith('✓') ? 'rgba(34,197,94,0.08)' : 'rgba(239,68,68,0.08)',
                border: `1px solid ${discogsTestResult.startsWith('✓') ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
                color: discogsTestResult.startsWith('✓') ? '#86efac' : '#fca5a5',
              }}>
                {discogsTestResult}
              </div>
            )}
          </div>

          <div style={{ padding: '12px 16px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.7 }}>
            <strong style={{ color: 'var(--text)' }}>How cover art works:</strong><br />
            When you open an album, BoogieBox checks the album's folder for a local image
            (<code>folder.jpg</code>, <code>cover.jpg</code>, <code>front.jpg</code>, etc.).
            If none is found and a Discogs token is configured, it searches Discogs and
            displays the cover from there. Local images are always preferred and served
            directly from your server — no data is sent to Discogs for those.
          </div>

          {showGeniusIntegration && <div style={{ ...P.sectionTitle, marginTop: 32 }}>Lyrics: Genius</div>}
          {showGeniusIntegration && <p style={{ color: 'var(--text-muted)', fontSize: 12, marginBottom: 20, lineHeight: 1.7 }}>
            Used to retrieve lyrics for tracks.
          </p>}

          {showGeniusIntegration && <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
              Create an app at{' '}
              <a href="https://genius.com/api-clients" target="_blank" rel="noreferrer"
                style={{ color: 'var(--accent)' }}>genius.com/api-clients</a>
              {' '}and copy the Client ID and Client Secret.
            </div>

            <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', flexWrap: 'wrap' }}>
              <input
                type="text"
                placeholder="Genius Client ID..."
                value={geniusClientId}
                onChange={e => { setGeniusClientId(e.target.value); setGeniusResult(null); }}
                style={{
                  flex: 1, minWidth: 220,
                  backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                  fontSize: 13, fontFamily: 'monospace', outline: 'none',
                }}
              />
              <input
                type="password"
                placeholder="Genius Client Secret..."
                value={geniusClientSecret}
                onChange={e => { setGeniusClientSecret(e.target.value); setGeniusResult(null); }}
                style={{
                  flex: 1, minWidth: 220,
                  backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                  fontSize: 13, fontFamily: 'monospace', outline: 'none',
                }}
              />
              <button onClick={testGeniusCreds} disabled={geniusSaving} style={{ ...P.btnSecondary, whiteSpace: 'nowrap' }}>
                Test
              </button>
              <button onClick={saveGeniusCreds} disabled={geniusSaving} style={{ ...P.btnPrimary, whiteSpace: 'nowrap' }}>
                {geniusSaving ? 'Saving...' : 'Save'}
              </button>
            </div>

            {geniusResult && (
              <div style={{
                marginTop: 10, padding: '8px 12px', borderRadius: 6, fontSize: 12,
                backgroundColor: geniusResult.startsWith('OK') ? 'rgba(34,197,94,0.08)' : 'rgba(239,68,68,0.08)',
                border: `1px solid ${geniusResult.startsWith('OK') ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
                color: geniusResult.startsWith('OK') ? '#86efac' : '#fca5a5',
              }}>
                {geniusResult}
              </div>
            )}
          </div>}

          {/* Last.fm */}
          <div style={{ ...P.sectionTitle, marginTop: 32 }}>Last.fm</div>
          <p style={{ color: 'var(--text-muted)', fontSize: 12, marginBottom: 20, lineHeight: 1.7 }}>
            Used to show artist biographies, album reviews, listener stats, and genre tags
            on artist and album pages in the Browse view. Requires a free API key.
          </p>

          <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
              To get a free API key:
              &nbsp;<strong style={{ color: 'var(--text)' }}>1.</strong> Create an account at{' '}
              <a href="https://www.last.fm/api/account/create" target="_blank" rel="noreferrer"
                style={{ color: 'var(--accent)' }}>last.fm/api/account/create</a>
              &nbsp;<strong style={{ color: 'var(--text)' }}>2.</strong> Fill in the application form
              &nbsp;<strong style={{ color: 'var(--text)' }}>3.</strong> Copy your API key and paste it below
            </div>

            <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', flexWrap: 'wrap' }}>
              <input
                type="password"
                placeholder="Paste your Last.fm API key…"
                value={lastfmKey}
                onChange={e => { setLastfmKey(e.target.value); setLastfmResult(null); }}
                style={{
                  flex: 1, minWidth: 240,
                  backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                  fontSize: 13, fontFamily: 'monospace', outline: 'none',
                }}
              />
              <button onClick={testLastfmKey} disabled={lastfmSaving} style={{ ...P.btnSecondary, whiteSpace: 'nowrap' }}>
                Test
              </button>
              <button onClick={saveLastfmKey} disabled={lastfmSaving} style={{ ...P.btnPrimary, whiteSpace: 'nowrap' }}>
                {lastfmSaving ? 'Saving…' : 'Save'}
              </button>
            </div>

            {lastfmResult && (
              <div style={{
                marginTop: 10, padding: '8px 12px', borderRadius: 6, fontSize: 12,
                backgroundColor: lastfmResult.startsWith('✓') ? 'rgba(34,197,94,0.08)' : 'rgba(239,68,68,0.08)',
                border: `1px solid ${lastfmResult.startsWith('✓') ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
                color: lastfmResult.startsWith('✓') ? '#86efac' : '#fca5a5',
              }}>
                {lastfmResult}
              </div>
            )}
          </div>

          <div style={{ ...P.sectionTitle, marginTop: 32 }}>Artist Images: Deezer + Spotify Fallback</div>
          <p style={{ color: 'var(--text-muted)', fontSize: 12, marginBottom: 20, lineHeight: 1.7 }}>
            Artist photos now use Deezer first when no local image is available.
            Discogs is used as secondary fallback (if token is configured), and Spotify is the final fallback.
          </p>

          <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
              Create an app at{' '}
              <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noreferrer"
                style={{ color: 'var(--accent)' }}>developer.spotify.com/dashboard</a>
              {' '}and copy the Client ID and Client Secret.
            </div>

            <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', flexWrap: 'wrap' }}>
              <input
                type="text"
                placeholder="Spotify Client ID..."
                value={spotifyClientId}
                onChange={e => { setSpotifyClientId(e.target.value); setSpotifyResult(null); }}
                style={{
                  flex: 1, minWidth: 220,
                  backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                  fontSize: 13, fontFamily: 'monospace', outline: 'none',
                }}
              />
              <input
                type="password"
                placeholder="Spotify Client Secret..."
                value={spotifyClientSecret}
                onChange={e => { setSpotifyClientSecret(e.target.value); setSpotifyResult(null); }}
                style={{
                  flex: 1, minWidth: 220,
                  backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                  fontSize: 13, fontFamily: 'monospace', outline: 'none',
                }}
              />
              <button onClick={testSpotifyCreds} disabled={spotifySaving} style={{ ...P.btnSecondary, whiteSpace: 'nowrap' }}>
                Test
              </button>
              <button onClick={saveSpotifyCreds} disabled={spotifySaving} style={{ ...P.btnPrimary, whiteSpace: 'nowrap' }}>
                {spotifySaving ? 'Saving...' : 'Save'}
              </button>
            </div>

            {spotifyResult && (
              <div style={{
                marginTop: 10, padding: '8px 12px', borderRadius: 6, fontSize: 12,
                backgroundColor: spotifyResult.startsWith('✓') ? 'rgba(34,197,94,0.08)' : 'rgba(239,68,68,0.08)',
                border: `1px solid ${spotifyResult.startsWith('✓') ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
                color: spotifyResult.startsWith('✓') ? '#86efac' : '#fca5a5',
              }}>
                {spotifyResult}
              </div>
            )}
          </div>

          {isAdmin && (
            <>
              <div style={{ ...P.sectionTitle, marginTop: 32 }}>Provider Usage</div>
              <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'flex-start', flexWrap: 'wrap', marginBottom: 14 }}>
                  <div>
                    <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                      Metadata provider usage
                    </div>
                    <div style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                      Counts increase only when BoogieBox actually caches provider-backed data or returns provider-backed data to the UI.
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 6 }}>
                      Snapshot: {providerUsage ? fmtQueueTime(providerUsage.fetched_at) : 'Not loaded'}
                    </div>
                  </div>
                  <button onClick={() => loadProviderUsage()} disabled={providerUsageLoading} style={{ ...P.btnSecondary, whiteSpace: 'nowrap' }}>
                    {providerUsageLoading ? 'Refreshing…' : 'Refresh Usage'}
                  </button>
                </div>

                {providerUsageError && (
                  <div style={{ fontSize: 12, color: '#ef4444', marginBottom: 12 }}>{providerUsageError}</div>
                )}

                {!providerUsageLoading && providerUsage && providerUsage.providers.length === 0 && (
                  <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>No provider usage has been recorded yet.</div>
                )}

                {!!providerUsage?.providers.length && (
                  <div style={{ display: 'grid', gap: 10, gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))' }}>
                    {providerUsage.providers.map((provider: ProviderUsageProviderSummary) => (
                      <div key={provider.provider} style={{ border: '1px solid var(--border)', borderRadius: 8, padding: '12px 14px', backgroundColor: 'color-mix(in srgb, var(--surface) 88%, var(--bg))' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8, alignItems: 'baseline' }}>
                          <div style={{ fontSize: 13, fontWeight: 700, color: 'var(--text)' }}>{formatProviderLabel(provider.provider)}</div>
                          <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--accent)', fontVariantNumeric: 'tabular-nums' }}>{provider.total_count}</div>
                        </div>
                        <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 8, lineHeight: 1.6 }}>
                          {Object.entries(provider.usage_breakdown)
                            .sort((a, b) => b[1] - a[1])
                            .map(([usageType, count]) => `${usageType.replace(/_/g, ' ')}: ${count}`)
                            .join(' • ')}
                        </div>
                        <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 8 }}>
                          Last used: {provider.last_used_at ? fmtQueueTime(provider.last_used_at) : 'Never'}
                        </div>
                      </div>
                    ))}
                  </div>
                )}

                {!!providerUsage?.rows.length && (
                  <div style={{ marginTop: 14 }}>
                    <button
                      onClick={() => setShowProviderUsageRows((value) => !value)}
                      style={{ ...P.btnSecondary, padding: '7px 14px', fontSize: 12 }}
                    >
                      {showProviderUsageRows ? 'Hide usage rows' : 'Show usage rows'}
                    </button>
                    {showProviderUsageRows && (
                      <div style={{ marginTop: 12, border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
                        {providerUsage.rows.map((row, index) => (
                          <div
                            key={`${row.provider}:${row.entity_type}:${row.usage_type}`}
                            style={{
                              display: 'grid',
                              gridTemplateColumns: 'minmax(92px, 110px) minmax(120px, 1fr) minmax(120px, 1fr) 72px 160px',
                              gap: 10,
                              padding: '10px 12px',
                              borderTop: index === 0 ? 'none' : '1px solid var(--border)',
                              fontSize: 11,
                              color: 'var(--text-muted)',
                            }}
                          >
                            <div style={{ color: 'var(--text)', fontWeight: 600 }}>{formatProviderLabel(row.provider)}</div>
                            <div>{row.entity_type}</div>
                            <div>{row.usage_type}</div>
                            <div style={{ textAlign: 'right', fontVariantNumeric: 'tabular-nums', color: 'var(--text)' }}>{row.count}</div>
                            <div style={{ textAlign: 'right' }}>{fmtQueueTime(row.last_used_at)}</div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </>
          )}

        </div>
      )}

      {/* ── Users Tab (admin only) ─────────────────────────────────────────── */}
      {activeTab === 'about' && (
        <div style={P.section}>
          <div style={P.sectionTitle}>About BoogieBox</div>
          <div style={P.aboutPanel}>
            <h3 style={P.aboutTitle}>BoogieBox</h3>
            <p style={P.aboutCopy}>
              BoogieBox is your self-hosted music library, with enough knobs for the careful archivist and enough groove for the couch DJ.
              It keeps your media close, your metadata tidy, and your evenings pleasantly over-engineered.
            </p>
            <a
              href="https://ko-fi.com/yronnen"
              target="_blank"
              rel="noreferrer"
              aria-label="Support BoogieBox on Ko-fi"
              style={P.aboutLink}
            >
              <img
                src="/support_me_on_kofi_dark.png"
                alt=""
                aria-hidden="true"
                style={P.aboutKofiImage}
              />
            </a>
          </div>
        </div>
      )}

      {activeTab === 'users' && isAdmin && (
        <div style={P.section}>
          <UserManagement currentUser={currentUser} />
        </div>
      )}
    </div>
  );
}

// ─── Styles ───────────────────────────────────────────────────────────────────

const P: Record<string, React.CSSProperties> = {
  page: {
    padding: '24px 32px', maxWidth: 860, flex: 1, overflowY: 'auto',
  },
  title: {
    fontSize: 20, fontWeight: 700, color: 'var(--text)',
    marginBottom: 20, letterSpacing: '-0.5px',
  },
  tabBar: {
    display: 'flex', gap: 2, marginBottom: 24,
    borderBottom: '1px solid var(--border)', paddingBottom: 0,
  },
  tab: {
    padding: '9px 18px', background: 'transparent', border: 'none',
    color: 'var(--text-muted)', cursor: 'pointer', fontSize: 13,
    fontFamily: 'inherit', borderBottomWidth: 2, borderBottomStyle: 'solid', borderBottomColor: 'transparent',
    marginBottom: -1, transition: 'color 0.15s',
  },
  tabActive: {
    color: 'var(--accent)', borderBottomColor: 'var(--accent)',
  },
  section: { paddingTop: 4 },
  sectionTitle: {
    fontSize: 11, fontWeight: 700, textTransform: 'uppercase',
    letterSpacing: 1, color: 'var(--text-muted)', marginBottom: 14,
  },
  btnPrimary: {
    backgroundColor: 'var(--accent)', color: '#fff', border: 'none',
    borderRadius: 6, padding: '9px 20px', cursor: 'pointer',
    fontSize: 13, fontFamily: 'inherit', fontWeight: 600,
  },
  btnSecondary: {
    backgroundColor: 'transparent', color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 6, padding: '9px 20px', cursor: 'pointer',
    fontSize: 13, fontFamily: 'inherit',
  },
  aboutPanel: {
    padding: '20px 24px',
    borderRadius: 8,
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)',
  },
  aboutTitle: {
    margin: '0 0 10px',
    fontSize: 18,
    fontWeight: 700,
    color: 'var(--text)',
  },
  aboutCopy: {
    margin: '0 0 18px',
    maxWidth: 620,
    color: 'var(--text-muted)',
    fontSize: 13,
    lineHeight: 1.7,
  },
  aboutLink: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 4,
    padding: 0,
    backgroundColor: 'transparent',
    lineHeight: 0,
    textDecoration: 'none',
  },
  aboutKofiImage: {
    display: 'block',
    width: 200,
    maxWidth: '100%',
    height: 'auto',
  },
};
