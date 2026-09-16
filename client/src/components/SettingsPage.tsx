/**
 * Defines the Settings Page React component and related UI helpers.
 */

import React, { useState, useEffect, useCallback } from 'react';
import { api, getStreamDirect, setStreamDirect } from '../api';
import type { AppSettings, ScanSchedule, WaveformMappingStatus, BpmAnalysisStatus, BoogieMixDeepAnalysisStatus, AuthUser, AdminQueueEntry, AdminQueueSnapshot, ClientEntityId, Library, AdminPostScanJobType, ProviderUsageSnapshot, ProviderUsageProviderSummary } from '../types';
import { DEFAULT_SETTINGS } from '../types';
import {
  hybridControlStyles,
  hybridSettingsStyles,
  HYBRID_FONT_FAMILY,
  HYBRID_THEME_MODES,
  type HybridThemeMode,
} from '../hybridPreview';
import { parseServerDate } from '../utils';
import LibrarySettingsTab from './LibrarySettingsTab';
import UserManagement from './UserManagement';
import FolderPickerModal from './FolderPickerModal';
import ConfirmModal from './ConfirmModal';

// ─── Helpers ─────────────────────────────────────────────────────────────────

export function fmtNextRun(iso: string | null): string {
  if (!iso) return 'Not scheduled';
  const d = parseServerDate(iso);
  if (!d || Number.isNaN(d.getTime())) return 'Not scheduled';
  const now = new Date();
  const diff = d.getTime() - now.getTime();
  if (diff < 0) return 'Overdue';
  const h = Math.floor(diff / 3600000);
  const m = Math.floor((diff % 3600000) / 60000);
  if (h > 23) return `in ${Math.floor(h / 24)}d ${h % 24}h`;
  if (h > 0)  return `in ${h}h ${m}m`;
  return `in ${m}m`;
}

export function fmtLastRun(iso: string | null): string {
  if (!iso) return 'Never';
  return (parseServerDate(iso) ?? new Date(iso)).toLocaleString();
}

export function formatBytes(bytes: number | null | undefined): string {
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

export function formatProviderLabel(provider: string): string {
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

export function formatQueueStateLabel(status: string): string {
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

type QueueIconKey = 'refresh' | 'artistImage' | 'albumImage' | 'cloud' | 'lyrics' | 'styles' | 'stop' | 'cancel' | 'code' | 'help' | 'checkCircle' | 'save';

const QueueIcon: Record<QueueIconKey, () => React.ReactElement> = {
  refresh: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="23 4 23 10 17 10" />
      <polyline points="1 20 1 14 7 14" />
      <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
    </svg>
  ),
  artistImage: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21v-1a7 7 0 0114 0v1" />
    </svg>
  ),
  albumImage: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="9" />
      <circle cx="12" cy="12" r="2.5" />
    </svg>
  ),
  cloud: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M17.5 19H7a4.5 4.5 0 01-.5-8.97A5.5 5.5 0 0117.24 8.03 4 4 0 0117.5 19z" />
    </svg>
  ),
  lyrics: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="4" y1="7" x2="20" y2="7" />
      <line x1="4" y1="12" x2="16" y2="12" />
      <line x1="4" y1="17" x2="12" y2="17" />
    </svg>
  ),
  styles: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M20.59 13.41L11 3.83A2 2 0 009.59 3.24L3.24 9.59A2 2 0 003.83 11l9.58 9.59a2 2 0 002.83 0l4.35-4.35a2 2 0 000-2.83z" />
      <circle cx="7.5" cy="7.5" r="1" fill="currentColor" stroke="none" />
    </svg>
  ),
  stop: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" stroke="none">
      <rect x="5" y="5" width="14" height="14" rx="2" />
    </svg>
  ),
  cancel: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  ),
  code: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="16 18 22 12 16 6" />
      <polyline points="8 6 2 12 8 18" />
    </svg>
  ),
  help: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="9" />
      <path d="M9.5 9a2.5 2.5 0 014.9.8c0 1.7-2.4 2-2.4 3.7" />
      <circle cx="12" cy="17" r="0.6" fill="currentColor" stroke="none" />
    </svg>
  ),
  checkCircle: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="9" />
      <polyline points="8 12.5 11 15.5 16 9" />
    </svg>
  ),
  save: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M5 3h11l3 3v15H5z" />
      <path d="M8 3v6h8V3" />
      <path d="M8 21v-7h8v7" />
    </svg>
  ),
};

const MUSIC_POST_SCAN_ACTIONS: Array<{ jobType: AdminPostScanJobType; label: string; icon: QueueIconKey }> = [
  { jobType: 'refresh_library_mappings', label: 'Refresh mappings', icon: 'refresh' },
  { jobType: 'cache_artist_images', label: 'Cache artist images', icon: 'artistImage' },
  { jobType: 'cache_album_images', label: 'Cache album images', icon: 'albumImage' },
  { jobType: 'warm_lastfm_info', label: 'Warm Last.fm', icon: 'cloud' },
  { jobType: 'warm_track_lyrics', label: 'Warm lyrics', icon: 'lyrics' },
  { jobType: 'sync_artist_styles', label: 'Sync artist styles', icon: 'styles' },
];

function getLibraryPostScanActions(library: Library): Array<{ jobType: AdminPostScanJobType; label: string; icon: QueueIconKey }> {
  return MUSIC_POST_SCAN_ACTIONS;
}

// ─── Color Swatch ─────────────────────────────────────────────────────────────

function ColorInput({ label, value, onChange }: { label: string; value: string; onChange: (v: string) => void }) {
  const inputLabel = `${label} custom color`;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flex: 1 }}>
        <input
          aria-label={`${label} color picker`}
          type="color"
          value={value}
          onChange={e => onChange(e.target.value)}
          style={{
            width: 40, height: 36, border: '1px solid var(--border)', borderRadius: 10,
            cursor: 'pointer', backgroundColor: 'var(--surface-subtle)', padding: 3,
          }}
        />
        <div>
          <div style={{ fontSize: 14, color: 'var(--text)', fontWeight: 500 }}>{label}</div>
          <div style={{ fontSize: 12, color: 'var(--text-muted)', fontFamily: 'monospace' }}>{value}</div>
        </div>
      </div>
      <input
        aria-label={inputLabel}
        type="text"
        value={value}
        onChange={e => onChange(e.target.value)}
        style={{
          ...hybridControlStyles.field,
          width: 108, minHeight: 36, padding: '7px 9px', fontSize: 13,
          fontFamily: 'monospace',
        }}
      />
    </div>
  );
}

// ─── Integration Connector ─────────────────────────────────────────────────

type ConnectorStatus = 'not_configured' | 'saved' | 'connected' | 'failed';

function connectorStatus(configured: boolean, connected: boolean | null): ConnectorStatus {
  if (!configured) return 'not_configured';
  if (connected === true) return 'connected';
  if (connected === false) return 'failed';
  return 'saved';
}

const CONNECTOR_STATUS_LABEL: Record<ConnectorStatus, string> = {
  not_configured: 'Not configured',
  saved: 'Saved',
  connected: 'Connected',
  failed: 'Failed',
};

const CONNECTOR_STATUS_COLOR: Record<ConnectorStatus, string> = {
  not_configured: 'var(--text-muted)',
  saved: 'var(--text)',
  connected: 'var(--success)',
  failed: 'var(--danger)',
};

function IntegrationConnector({
  dotColor,
  name,
  description,
  status,
  helpOpen,
  onToggleHelp,
  helpContent,
  onTest,
  onSave,
  busy,
  errorMessage,
  children,
}: {
  dotColor: string;
  name: string;
  description: string;
  status: ConnectorStatus;
  helpOpen: boolean;
  onToggleHelp: () => void;
  helpContent: React.ReactNode;
  onTest: () => void;
  onSave: () => void;
  busy: boolean;
  errorMessage: string | null;
  children: React.ReactNode;
}) {
  return (
    <div style={{ padding: '14px 16px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 12 }}>
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
          <span style={{ width: 9, height: 9, borderRadius: '50%', backgroundColor: dotColor, flexShrink: 0 }} />
          <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)' }}>{name}</div>
          <span style={{ fontSize: 12, color: CONNECTOR_STATUS_COLOR[status], border: '1px solid var(--border)', borderRadius: 999, padding: '2px 8px', whiteSpace: 'nowrap' }}>
            {CONNECTOR_STATUS_LABEL[status]}
          </span>
        </div>
        <div style={{ display: 'flex', gap: 6, flexShrink: 0 }}>
          <button
            type="button"
            onClick={onToggleHelp}
            style={{ ...hybridControlStyles.iconButton, ...(helpOpen ? { color: 'var(--accent)', borderColor: 'var(--accent)' } : {}) }}
            title={helpOpen ? 'Hide setup instructions' : 'Setup instructions'}
            aria-label={helpOpen ? 'Hide setup instructions' : 'Setup instructions'}
          >
            <QueueIcon.help />
          </button>
          <button
            type="button"
            onClick={onTest}
            disabled={busy}
            style={{ ...hybridControlStyles.iconButton, ...(busy ? { opacity: 0.6 } : {}) }}
            title="Test connection"
            aria-label={`Test ${name} connection`}
          >
            <QueueIcon.checkCircle />
          </button>
          <button
            type="button"
            onClick={onSave}
            disabled={busy}
            style={{ ...hybridControlStyles.iconButton, color: 'var(--accent)', ...(busy ? { opacity: 0.6 } : {}) }}
            title="Save"
            aria-label={`Save ${name} credentials`}
          >
            <QueueIcon.save />
          </button>
        </div>
      </div>
      <p style={{ color: 'var(--text-muted)', fontSize: 14, lineHeight: 1.6, margin: '8px 0 0' }}>{description}</p>
      {helpOpen && (
        <div style={{ marginTop: 10, padding: '10px 12px', borderRadius: 6, backgroundColor: 'var(--bg)', border: '1px solid var(--border)', fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 }}>
          {helpContent}
        </div>
      )}
      <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start', flexWrap: 'wrap', marginTop: 12 }}>
        {children}
      </div>
      {errorMessage && (
        <div style={{ marginTop: 8, fontSize: 14, color: 'var(--danger)' }}>{errorMessage}</div>
      )}
    </div>
  );
}

/** Folds a legacy "✓ …" / "✗ …" / "Testing…" result string into: nothing (success
 * already shows via the status pill), a muted in-progress note, or an error line. */
function connectorMessage(result: string | null): string | null {
  if (!result) return null;
  if (result.startsWith('✓')) return null;
  if (/^Testing/.test(result)) return result;
  return result.replace(/^✗\s*/, '');
}

// ─── Advanced-tab shared controls ──────────────────────────────────────────

function ToggleSwitch({
  checked,
  onChange,
  ariaLabel,
  disabled,
}: {
  checked: boolean;
  onChange: () => void;
  ariaLabel: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={ariaLabel}
      aria-checked={checked}
      onClick={onChange}
      disabled={disabled}
      style={{
        ...hybridControlStyles.switchTrack,
        ...(checked ? hybridControlStyles.switchTrackActive : {}),
        ...(disabled ? { opacity: 0.6, cursor: 'default' } : {}),
        flexShrink: 0,
      }}
    >
      <span style={{ ...hybridControlStyles.switchThumb, ...(checked ? hybridControlStyles.switchThumbActive : {}) }} />
    </button>
  );
}

function ToggleRow({
  title,
  description,
  checked,
  onChange,
  ariaLabel,
  disabled,
}: {
  title: string;
  description: React.ReactNode;
  checked: boolean;
  onChange: () => void;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 16 }}>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>{title}</div>
        <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 }}>{description}</div>
      </div>
      <ToggleSwitch checked={checked} onChange={onChange} ariaLabel={ariaLabel ?? title} disabled={disabled} />
    </div>
  );
}

/** Renders a card's busy/result feedback line consistently: muted while busy,
 * success-toned otherwise, danger-toned for a result starting with "Error". */
function InlineStatus({ busy, busyText, result }: { busy: boolean; busyText: string; result: string | null }) {
  if (busy) return <div style={{ fontSize: 14, color: 'var(--text-muted)', marginTop: 6 }}>{busyText}</div>;
  if (!result) return null;
  const isError = result.startsWith('Error') || result.startsWith('✗');
  return <div style={{ fontSize: 14, color: isError ? 'var(--danger)' : 'var(--success)', marginTop: 6 }}>{result}</div>;
}

function SettingsPanel({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <section aria-label={title} style={hybridSettingsStyles.panel}>
      <div style={hybridSettingsStyles.panelHeader}>
        <div>
          <div style={hybridSettingsStyles.sectionTitle}>{title}</div>
          {description && <div style={hybridSettingsStyles.panelDescription}>{description}</div>}
        </div>
      </div>
      {children}
    </section>
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
        <div style={{ fontWeight: 600, fontSize: 15, color: 'var(--text)', marginBottom: 2 }}>
          {library.name}
        </div>
        <div style={{ fontSize: 13, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {library.primary_path ?? library.path}
        </div>
        {schedule && (
          <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 4, display: 'flex', gap: 16 }}>
            <span>Last: {fmtLastRun(schedule.last_run)}</span>
            {schedule.enabled ? <span>Next: {fmtNextRun(schedule.next_run)}</span> : null}
          </div>
        )}
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexShrink: 0 }}>
        {/* Enable toggle */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 14, color: 'var(--text-muted)' }}>
          <button
            type="button"
            role="switch"
            aria-label={`Auto-scan ${library.name}`}
            aria-checked={enabled}
            onClick={() => toggle(!enabled)}
            style={{ ...hybridControlStyles.switchTrack, ...(enabled ? hybridControlStyles.switchTrackActive : {}) }}
          >
            <span style={{ ...hybridControlStyles.switchThumb, ...(enabled ? hybridControlStyles.switchThumbActive : {}) }} />
          </button>
          {enabled ? 'On' : 'Off'}
        </div>

        {/* Frequency */}
        <select
          value={freq}
          onChange={e => changeFreq(Number(e.target.value))}
          disabled={!enabled}
          style={{
            ...hybridControlStyles.select, color: enabled ? 'var(--text)' : 'var(--text-muted)',
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
    padding: '14px 16px', borderRadius: 12, marginBottom: 8,
    backgroundColor: 'var(--surface)', border: '1px solid var(--divider-subtle)',
  } as React.CSSProperties,
  saveBtn: {
    ...hybridControlStyles.primaryButton,
    minHeight: 34,
    padding: '6px 12px',
  } as React.CSSProperties,
  deleteBtn: {
    ...hybridControlStyles.iconButton,
    width: 34,
    minWidth: 34,
    height: 34,
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
  hideCompilationOnlyArtists?: boolean;
  onHideCompilationOnlyArtistsChange?: (enabled: boolean) => void;
  hybridThemeMode?: HybridThemeMode;
  onHybridThemeModeChange?: (mode: HybridThemeMode) => void;
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
  hideCompilationOnlyArtists = true,
  onHideCompilationOnlyArtistsChange,
  hybridThemeMode = 'dark',
  onHybridThemeModeChange,
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
  const canManageLibraries = isAdmin || currentUser.canManageLibraries;
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
  const [discogsConnected, setDiscogsConnected] = useState<boolean | null>(null);
  const [discogsHelpOpen, setDiscogsHelpOpen] = useState(false);
  const [spotifyClientId, setSpotifyClientId] = useState('');
  const [spotifyClientSecret, setSpotifyClientSecret] = useState('');
  const [spotifySaving, setSpotifySaving] = useState(false);
  const [spotifyResult, setSpotifyResult] = useState<string | null>(null);
  const [spotifyConnected, setSpotifyConnected] = useState<boolean | null>(null);
  const [spotifyHelpOpen, setSpotifyHelpOpen] = useState(false);
  const [geniusClientId, setGeniusClientId] = useState('');
  const [geniusClientSecret, setGeniusClientSecret] = useState('');
  const [geniusSaving, setGeniusSaving] = useState(false);
  const [geniusResult, setGeniusResult] = useState<string | null>(null);
  const [lastfmKey, setLastfmKey] = useState('');
  const [lastfmSaving, setLastfmSaving] = useState(false);
  const [lastfmResult, setLastfmResult] = useState<string | null>(null);
  const [lastfmConnected, setLastfmConnected] = useState<boolean | null>(null);
  const [lastfmHelpOpen, setLastfmHelpOpen] = useState(false);
  const [streamDirect, setStreamDirectState] = useState(() => getStreamDirect());
  const [transcodeQuality, setTranscodeQuality] = useState<'low' | 'high'>(settings.transcodeQuality === 'high' ? 'high' : 'low');
  const [transcodeQualitySaving, setTranscodeQualitySaving] = useState(false);
  const [transcodeQualityResult, setTranscodeQualityResult] = useState<string | null>(null);
  const [replayGainEnabled, setReplayGainEnabled] = useState(settings.replayGainEnabled === 'true');

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
  const [deepmixDebugLoggingEnabled, setDeepmixDebugLoggingEnabled] = useState(settings.deepmixDebugLoggingEnabled === 'true');
  const [deepmixDebugSaving, setDeepmixDebugSaving] = useState(false);
  const [deepmixDebugResult, setDeepmixDebugResult] = useState<string | null>(null);
  const [boogiemixOutputFolder, setBoogiemixOutputFolder] = useState(settings.boogiemixOutputFolder || '');
  const [boogiemixOutputFolderSaving, setBoogiemixOutputFolderSaving] = useState(false);
  const [boogiemixOutputFolderResult, setBoogiemixOutputFolderResult] = useState<string | null>(null);
  const [boogiemixDeepStatus, setBoogiemixDeepStatus] = useState<BoogieMixDeepAnalysisStatus | null>(null);
  const [boogiemixDeepLoading, setBoogiemixDeepLoading] = useState(false);
  const [boogiemixDeepBackgroundMode, setBoogiemixDeepBackgroundMode] = useState(settings.boogiemixDeepAnalysisBackgroundMode || 'off');
  const [boogiemixDeepPauseBackground, setBoogiemixDeepPauseBackground] = useState(settings.boogiemixDeepAnalysisPauseBackground === 'true');
  const [boogiemixDeepMaxDurationMins, setBoogiemixDeepMaxDurationMins] = useState(Number(settings.boogiemixDeepAnalysisMaxDurationMins) || 15);
  const [boogiemixDeepModel, setBoogiemixDeepModel] = useState(settings.boogiemixDeepAnalysisModel || 'mdx_extra_q');
  const [boogiemixHighQualityWaitMs, setBoogiemixHighQualityWaitMs] = useState(Number(settings.boogiemixHighQualityWaitMs) || 20000);
  const [boogiemixBpmWaitMs, setBoogiemixBpmWaitMs] = useState(Number(settings.boogiemixBpmWaitMs) || 15000);
  const [boogiemixWaveformWaitMs, setBoogiemixWaveformWaitMs] = useState(Number(settings.boogiemixWaveformWaitMs) || 15000);
  const [boogiemixDeepActionBusy, setBoogiemixDeepActionBusy] = useState<string | null>(null);
  const [boogiemixDeepActionResult, setBoogiemixDeepActionResult] = useState<string | null>(null);
  const [boogiemixDeepSelectedLibrary, setBoogiemixDeepSelectedLibrary] = useState<ClientEntityId | ''>('');
  // A single slot for "the confirm dialog currently open" — only one of this
  // page's several destructive actions can be mid-confirmation at a time.
  const [pendingConfirm, setPendingConfirm] = useState<{
    title: string;
    message: string;
    confirmLabel?: string;
    tone?: 'default' | 'danger';
    onConfirm: () => void;
  } | null>(null);
  const showGeniusIntegration = false;
  const [currentDbFolder, setCurrentDbFolder] = useState<string>('');
  const [logFilePaths, setLogFilePaths] = useState<{ server?: string; scan?: string; deep?: string }>({});
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
    setWaveformGenerateOnMissing(settings.waveformGenerateOnMissing !== 'false');
    setWaveformBackgroundEnabled(settings.waveformBackgroundEnabled === 'true');
    setWaveformFrequencyHours(Number(settings.waveformBackgroundFrequencyHours) || 24);
    setWaveformBatchSize(Number(settings.waveformBackgroundBatchSize) || 100);
    setBpmBackgroundEnabled(settings.bpmBackgroundEnabled === 'true');
    setBpmFrequencyHours(Number(settings.bpmBackgroundFrequencyHours) || 24);
    setScanDebugLoggingEnabled(settings.scanDebugLoggingEnabled === 'true');
    setDeepmixDebugLoggingEnabled(settings.deepmixDebugLoggingEnabled === 'true');
    setBoogiemixOutputFolder(settings.boogiemixOutputFolder || '');
    setBoogiemixDeepBackgroundMode(settings.boogiemixDeepAnalysisBackgroundMode || 'off');
    setBoogiemixDeepPauseBackground(settings.boogiemixDeepAnalysisPauseBackground === 'true');
    setBoogiemixDeepMaxDurationMins(Number(settings.boogiemixDeepAnalysisMaxDurationMins) || 15);
    setBoogiemixDeepModel(settings.boogiemixDeepAnalysisModel || 'mdx_extra_q');
    setBoogiemixHighQualityWaitMs(Number(settings.boogiemixHighQualityWaitMs) || 20000);
    setBoogiemixBpmWaitMs(Number(settings.boogiemixBpmWaitMs) || 15000);
    setBoogiemixWaveformWaitMs(Number(settings.boogiemixWaveformWaitMs) || 15000);
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
    if (activeTab === 'libraries' && canManageLibraries) refreshLibraries().catch(() => {});
    if (activeTab === 'schedules' && canManageLibraries) loadSchedules();
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
        setWaveformGenerateOnMissing((s.waveformGenerateOnMissing ?? 'true') === 'true');
        setWaveformBackgroundEnabled((s.waveformBackgroundEnabled ?? 'false') === 'true');
        setWaveformFrequencyHours(Number(s.waveformBackgroundFrequencyHours ?? '24') || 24);
        setWaveformBatchSize(Number(s.waveformBackgroundBatchSize ?? '100') || 100);
        setBpmBackgroundEnabled((s.bpmBackgroundEnabled ?? 'false') === 'true');
        setBpmFrequencyHours(Number(s.bpmBackgroundFrequencyHours ?? '24') || 24);
        setScanDebugLoggingEnabled((s.scanDebugLoggingEnabled ?? 'false') === 'true');
        setDeepmixDebugLoggingEnabled((s.deepmixDebugLoggingEnabled ?? 'false') === 'true');
        setBoogiemixOutputFolder(s.boogiemixOutputFolder ?? '');
        setBoogiemixDeepBackgroundMode(s.boogiemixDeepAnalysisBackgroundMode ?? 'off');
        setBoogiemixDeepPauseBackground((s.boogiemixDeepAnalysisPauseBackground ?? 'false') === 'true');
        setBoogiemixDeepMaxDurationMins(Number(s.boogiemixDeepAnalysisMaxDurationMins ?? '15') || 15);
        setBoogiemixDeepModel(s.boogiemixDeepAnalysisModel || 'mdx_extra_q');
        setBoogiemixHighQualityWaitMs(Number(s.boogiemixHighQualityWaitMs) || 20000);
        setBoogiemixBpmWaitMs(Number(s.boogiemixBpmWaitMs) || 15000);
        setBoogiemixWaveformWaitMs(Number(s.boogiemixWaveformWaitMs) || 15000);
      });
      refreshLibraries().catch(() => {});
      loadDlnaStatus();
      loadWaveformStatus();
      loadBpmStatus();
      loadBoogieMixDeepStatus(true);
      api.systemStatus().then(s => {
        if (s.dbFolder) setCurrentDbFolder(s.dbFolder);
        setLogFilePaths({ server: s.logFile, scan: s.scanDebugLogFile, deep: s.deepDebugLogFile });
      }).catch(() => {});
    }
  }, [activeTab, isAdmin, canManageLibraries, refreshLibraries, loadSchedules, loadQueueSnapshot, loadDlnaStatus, loadWaveformStatus, loadBpmStatus, loadBoogieMixDeepStatus, loadProviderUsage]);

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
    onHybridThemeModeChange?.('custom');
  };

  const applyPreset = (preset: Partial<AppSettings>) => {
    const hybridPreset = { ...preset };
    delete hybridPreset.fontFamily;
    const next = { ...local, bgTexture: 'none', ...hybridPreset };
    setLocal(next);
    onSettingsChange(next);
    onHybridThemeModeChange?.('custom');
  };

  const resetTheme = () => {
    const next = { ...local, ...ORIGINAL_THEME };
    setLocal(next);
    onSettingsChange(next);
    onHybridThemeModeChange?.('custom');
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

  const saveDeepmixDebugSettings = async (enabled: boolean) => {
    setDeepmixDebugSaving(true);
    setDeepmixDebugResult(null);
    try {
      await api.settings.update({ deepmixDebugLoggingEnabled: enabled ? 'true' : 'false' });
      setDeepmixDebugLoggingEnabled(enabled);
      setDeepmixDebugResult(enabled ? 'Debug logging enabled' : 'Debug logging disabled');
    } catch (e: any) {
      setDeepmixDebugLoggingEnabled(!enabled);
      setDeepmixDebugResult(`Error: ${e.message}`);
    } finally {
      setDeepmixDebugSaving(false);
      setTimeout(() => setDeepmixDebugResult(null), 3000);
    }
  };

  const saveDiscogsToken = async () => {
    setDiscogsSaving(true);
    setDiscogsTestResult(null);
    setDiscogsConnected(null);
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
        setDiscogsConnected(true);
      } else {
        const j = await resp.json().catch(() => ({}));
        setDiscogsTestResult(`✗ Discogs returned ${resp.status}: ${(j as any).message ?? 'error'}`);
        setDiscogsConnected(false);
      }
    } catch (e: any) {
      setDiscogsTestResult('✗ Network error: ' + e.message);
      setDiscogsConnected(false);
    } finally {
      setDiscogsSaving(false);
    }
  };

  const saveSpotifyCreds = async () => {
    setSpotifySaving(true);
    setSpotifyResult(null);
    setSpotifyConnected(null);
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
      setSpotifyConnected(true);
    } catch (e: any) {
      setSpotifyResult('✗ ' + e.message);
      setSpotifyConnected(false);
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
    setLastfmConnected(null);
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
        setLastfmConnected(false);
      } else {
        setLastfmResult('✓ Key is valid — Last.fm connection OK');
        setLastfmConnected(true);
      }
    } catch (e: any) {
      setLastfmResult('✗ Network error: ' + e.message);
      setLastfmConnected(false);
    } finally {
      setLastfmSaving(false);
    }
  };

  return (
    <div data-ui-design="hybrid" data-ui-region="settings" style={P.page}>
      <div style={P.pageHeader}>
        <div>
          <h2 style={P.title}>Settings</h2>
          <div style={P.subtitle}>Personalize BoogieBox and manage this server.</div>
        </div>
        <div style={P.account}>
          <span>{currentUser.username}</span>
          <span style={P.roleBadge}>{currentUser.role}</span>
          <button type="button" onClick={onLogout} style={P.logoutButton}>Log out</button>
        </div>
      </div>

      {/* Tab bar */}
      <div role="tablist" aria-label="Settings sections" style={P.tabBar}>
        {([
          ['theme',        'User Settings'],
          ...(canManageLibraries ? [
            ['libraries',    'Libraries'],
            ['schedules',    'Auto-Scan'],
          ] : []),
          ...(isAdmin ? [
            ['integrations', 'Integrations'],
            ['advanced',     'Advanced'],
            ['users',        'Users'],
          ] : []),
          ['about',        'About'],
        ] as [string, string][]).map(([t, label]) => (
          <button
            key={t}
            type="button"
            role="tab"
            aria-selected={activeTab === t}
            style={{ ...P.tab, ...(activeTab === t ? P.tabActive : {}) }}
            onClick={() => setActiveTab(t as any)}
          >
            {label}
          </button>
        ))}
      </div>

      {/* ── Theme Tab ─────────────────────────────────────────────────────── */}
      {activeTab === 'libraries' && canManageLibraries && (
        <div style={P.section}>
          <LibrarySettingsTab libraries={libraries as Library[]} onRefresh={refreshLibraries} />
        </div>
      )}

      {activeTab === 'theme' && (
        <div style={P.section}>
          <SettingsPanel
            title="Theme mode"
            description="Light and Dark use supported New-design defaults. Custom keeps your own saved palette."
          >
            <div role="group" aria-label="New design theme mode" style={hybridControlStyles.segmentedGroup}>
              {HYBRID_THEME_MODES.map(mode => {
                const active = hybridThemeMode === mode;
                return (
                  <button
                    key={mode}
                    type="button"
                    aria-label={`Use ${mode} theme`}
                    aria-pressed={active}
                    onClick={() => onHybridThemeModeChange?.(mode)}
                    style={{
                      ...hybridControlStyles.segment,
                      ...(active ? hybridControlStyles.segmentActive : {}),
                      minWidth: 78,
                    }}
                  >
                    {mode.charAt(0).toUpperCase() + mode.slice(1)}
                  </button>
                );
              })}
            </div>
          </SettingsPanel>

          <SettingsPanel
            title="Custom palette"
            description="Choosing a preset or editing a color switches the New design to Custom without changing your saved font."
          >
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginBottom: 20 }}>
              {THEME_PRESETS.map(preset => (
                <button
                  key={preset.label}
                  type="button"
                  onClick={() => applyPreset(preset.settings)}
                  style={{
                    ...hybridControlStyles.secondaryButton,
                    minHeight: 34,
                    padding: '7px 12px',
                    background: preset.settings.colorBg ?? 'var(--surface-subtle)',
                    color: preset.settings.colorText ?? 'var(--text)',
                  }}
                >
                  {preset.label}
                </button>
              ))}
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(240px, 1fr))', gap: '14px 32px' }}>
              <ColorInput label="Background" value={local.colorBg} onChange={v => set('colorBg', v)} />
              <ColorInput label="Surface" value={local.colorSurface} onChange={v => set('colorSurface', v)} />
              <ColorInput label="Border" value={local.colorBorder} onChange={v => set('colorBorder', v)} />
              <ColorInput label="Accent" value={local.colorAccent} onChange={v => set('colorAccent', v)} />
              <ColorInput label="Text" value={local.colorText} onChange={v => set('colorText', v)} />
              <ColorInput label="Muted Text" value={local.colorTextMuted} onChange={v => set('colorTextMuted', v)} />
            </div>
          </SettingsPanel>

          <SettingsPanel
            title="Typeface & preview"
            description="Satoshi is fixed for the New design. Your previous font setting remains stored during migration."
          >
            <div style={{ display: 'grid', gridTemplateColumns: 'minmax(180px, 0.65fr) minmax(300px, 1.35fr)', gap: 14 }}>
              <div style={{ padding: '16px', borderRadius: 12, background: 'var(--surface-subtle)' }}>
                <div style={{ fontFamily: HYBRID_FONT_FAMILY, color: 'var(--text)', fontSize: 20, fontWeight: 750 }}>
                  Satoshi
                </div>
                <div style={{ color: 'var(--text-muted)', fontSize: 13, marginTop: 5 }}>New design typeface</div>
              </div>
              <div style={{
                padding: 18,
                borderRadius: 12,
                backgroundColor: local.colorSurface,
                border: `1px solid ${local.colorBorder}`,
                fontFamily: HYBRID_FONT_FAMILY,
              }}>
                <div style={{ color: local.colorText, fontWeight: 700, fontSize: 17, marginBottom: 6 }}>
                  The Quick Brown Fox
                </div>
                <div style={{ color: local.colorTextMuted, fontSize: 14, marginBottom: 10 }}>
                  Artist · Album · 2024 · Jazz
                </div>
                <div style={{ display: 'flex', gap: 8 }}>
                  <div style={{ padding: '6px 14px', borderRadius: 8, backgroundColor: local.colorAccent, color: 'var(--on-accent)', fontSize: 14, fontWeight: 650 }}>
                    ▶ Play
                  </div>
                  <div style={{ padding: '6px 14px', borderRadius: 8, border: `1px solid ${local.colorBorder}`, color: local.colorTextMuted, fontSize: 14 }}>
                    + Queue
                  </div>
                </div>
              </div>
            </div>
          </SettingsPanel>

          <SettingsPanel
            title="Accent source"
            description="Adaptive follows album and artist artwork; turning it off uses the selected theme accent everywhere."
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              <button
                type="button"
                role="switch"
                aria-checked={adaptiveAccentEnabled}
                aria-label="Adaptive accent"
                onClick={() => onAdaptiveAccentEnabledChange?.(!adaptiveAccentEnabled)}
                title={adaptiveAccentEnabled ? 'Adaptive accent is enabled' : 'Selected theme accent is enabled'}
                style={{ ...hybridControlStyles.switchTrack, ...(adaptiveAccentEnabled ? hybridControlStyles.switchTrackActive : {}) }}
              >
                <span style={{ ...hybridControlStyles.switchThumb, ...(adaptiveAccentEnabled ? hybridControlStyles.switchThumbActive : {}) }} />
              </button>
              <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.5 }}>
                {adaptiveAccentEnabled
                  ? 'Adaptive from album/artist artwork (current behavior)'
                  : 'Use selected theme accent only'}
              </div>
            </div>
            <div style={{ color: 'var(--text-muted)', fontSize: 13, marginTop: 6 }}>
              Applies to album view, artist view, and playback bar. Saved for your user profile.
            </div>
          </SettingsPanel>

          <SettingsPanel
            title="Artist browsing"
            description="Many library entries are artists with no releases of their own — they only appear via a track or two on someone else's compilation. Hide them from Browse to keep the artist list focused on artists you actually own releases from. A direct search always finds them regardless of this setting."
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              <button
                type="button"
                role="switch"
                aria-checked={hideCompilationOnlyArtists}
                aria-label="Hide compilation-only artists"
                onClick={() => onHideCompilationOnlyArtistsChange?.(!hideCompilationOnlyArtists)}
                title={hideCompilationOnlyArtists ? 'Compilation-only artists are hidden from Browse' : 'All artists are shown in Browse'}
                style={{ ...hybridControlStyles.switchTrack, ...(hideCompilationOnlyArtists ? hybridControlStyles.switchTrackActive : {}) }}
              >
                <span style={{ ...hybridControlStyles.switchThumb, ...(hideCompilationOnlyArtists ? hybridControlStyles.switchThumbActive : {}) }} />
              </button>
              <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.5 }}>
                {hideCompilationOnlyArtists
                  ? 'Hiding artists with no owned releases (default)'
                  : 'Showing every artist, including compilation-only appearances'}
              </div>
            </div>
            <div style={{ color: 'var(--text-muted)', fontSize: 13, marginTop: 6 }}>
              Applies to Browse only. Saved for your user profile.
            </div>
          </SettingsPanel>

          <SettingsPanel
            title="Vinyl Mode"
            description="Turn vinyl mode on/off from the player. These preferences apply whenever it's active."
          >
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 14, cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={vinylHardcore}
                  onChange={(e) => onVinylHardcoreChange?.(e.target.checked)}
                />
                Hardcore Vinyl (no seeking / no jumping)
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 14, cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={vinylNeedleDrop}
                  onChange={(e) => onVinylNeedleDropChange?.(e.target.checked)}
                />
                Needle-drop sound
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 14, cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={vinylAnalogFxDisabled}
                  onChange={(e) => onVinylAnalogFxDisabledChange?.(e.target.checked)}
                />
                Disable analog noise effects
              </label>
              <div style={{ opacity: vinylAnalogFxDisabled ? 0.5 : 1 }}>
                <div style={{ fontSize: 14, color: 'var(--text-muted)', marginBottom: 4 }}>
                  Needle-drop intensity: {Math.round(vinylNeedleDropIntensity * 100)}%
                </div>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={1}
                  value={Math.round(vinylNeedleDropIntensity * 100)}
                  disabled={vinylAnalogFxDisabled}
                  onChange={(e) => onVinylNeedleDropIntensityChange?.(Math.max(0, Math.min(1, Number(e.target.value) / 100)))}
                  style={{ width: 260 }}
                />
              </div>
            </div>
            <div style={{ color: 'var(--text-muted)', fontSize: 13, marginTop: 6 }}>
              Saved for your user profile.
            </div>
          </SettingsPanel>

          <div style={{ display: 'flex', gap: 12, alignItems: 'center', justifyContent: 'space-between' }}>
            <div style={{ color: 'var(--text-muted)', fontSize: 13 }}>
              Theme, Custom palette, and accent settings auto-save for your user profile.
            </div>
            <button type="button" style={P.btnSecondary} onClick={resetTheme}>
              Reset to Default
            </button>
          </div>
        </div>
      )}

      {/* ── Schedules Tab ─────────────────────────────────────────────────── */}
      {activeTab === 'schedules' && canManageLibraries && (
        <div style={P.section}>
          <div style={P.sectionTitle}>Auto-Scan Schedule</div>
          <p style={{ color: 'var(--text-muted)', fontSize: 14, marginBottom: 20, lineHeight: 1.6 }}>
            Set how often each library should be checked for new files. Scans run in the
            background and schedule changes take effect right away.
          </p>

          {libraries.length === 0 && (
            <div style={{ color: 'var(--text-muted)', fontSize: 15, padding: '20px 0' }}>
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
            <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 10 }}>
              <div>
                <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)' }}>Queue &amp; Maintenance</div>
                <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 4 }}>
                  Monitor background activity, stop stuck scan work, and queue maintenance tasks when you need them.
                </div>
              </div>
              <div style={{ display: 'flex', gap: 8, flexShrink: 0 }}>
                <button
                  type="button"
                  onClick={() => void loadQueueSnapshot()}
                  disabled={queueLoading}
                  style={{ ...hybridControlStyles.iconButton, ...(queueLoading ? { opacity: 0.6 } : {}) }}
                  title={queueLoading ? 'Refreshing…' : 'Refresh'}
                  aria-label={queueLoading ? 'Refreshing' : 'Refresh queue snapshot'}
                >
                  <QueueIcon.refresh />
                </button>
                <button
                  type="button"
                  onClick={() => setShowRawQueueSnapshot((value) => !value)}
                  style={{
                    ...hybridControlStyles.iconButton,
                    ...(showRawQueueSnapshot ? { color: 'var(--accent)', borderColor: 'var(--accent)' } : {}),
                  }}
                  title={showRawQueueSnapshot ? 'Hide raw queue snapshot' : 'Show raw queue snapshot'}
                  aria-label={showRawQueueSnapshot ? 'Hide raw queue snapshot' : 'Show raw queue snapshot'}
                >
                  <QueueIcon.code />
                </button>
              </div>
            </div>
            {queueActionResult && (
              <div style={{ color: '#86efac', fontSize: 13, marginBottom: 10 }}>
                {queueActionResult}
              </div>
            )}
            {queueError && (
              <div style={{ color: '#fca5a5', fontSize: 13, marginBottom: 10 }}>
                {queueError}
              </div>
            )}
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
                  fontSize: 13,
                  lineHeight: 1.6,
                  fontFamily: 'Consolas, Monaco, monospace',
                  marginBottom: 12,
                }}
              />
            )}
            {queueSnapshot && (
              <div style={{ display: 'grid', gap: 14 }}>
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
                        color: 'var(--text-muted)',
                        fontSize: 13,
                      }}
                    >
                      <strong style={{ color: 'var(--text)', fontWeight: 600 }}>{item.count}</strong> {item.label}
                    </div>
                  ))}
                </div>
                <div>
                  <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 8 }}>Live queue</div>
                  {(queueSnapshot.queues.scan.length > 0 || queueSnapshot.queues.postScan.length > 0) ? (
                  <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
                    {[
                      ...queueSnapshot.queues.scan.map((entry) => {
                        const busyKey = `scan:${entry.id}`;
                        const isRunning = entry.status === 'running';
                        return {
                          key: `scan-${entry.id}`,
                          title: `${entry.library_name || 'Library'} scan`,
                          status: entry.status,
                          meta: [
                            entry.files_found != null || entry.files_scanned != null
                              ? `${entry.files_scanned ?? 0} of ${entry.files_found ?? 0} files scanned`
                              : 'Preparing scan',
                            entry.started_at ? `Started ${fmtQueueTime(entry.started_at)}` : null,
                            entry.errors != null ? `Errors ${entry.errors}` : null,
                          ].filter(Boolean).join(' • '),
                          busyKey,
                          actionTitle: isRunning ? 'Stop scan' : 'Cancel scan',
                          onAction: () => void runQueueAction(busyKey, `Scan job ${entry.id} cancelled.`, () => api.admin.cancelScanJob(entry.id)),
                        };
                      }),
                      ...queueSnapshot.queues.postScan.map((entry) => {
                        const isRunning = entry.status === 'running';
                        const busyKey = `post-scan:${entry.id}`;
                        const action = isRunning
                          ? () => api.admin.failPostScanJob(entry.id)
                          : () => api.admin.cancelPostScanJob(entry.id);
                        const successMessage = isRunning
                          ? `Post-scan job ${entry.id} stopped.`
                          : `Post-scan job ${entry.id} cancelled.`;
                        return {
                          key: `post-scan-${entry.id}`,
                          title: `${entry.library_name || 'Library'} ${entry.job_type ? `• ${entry.job_type}` : 'post-scan'}`,
                          status: entry.status,
                          meta: [
                            entry.current_step || 'Waiting for worker',
                            entry.started_at ? `Started ${fmtQueueTime(entry.started_at)}` : null,
                            entry.error_message || null,
                          ].filter(Boolean).join(' • '),
                          busyKey,
                          actionTitle: isRunning ? 'Stop task' : 'Cancel task',
                          onAction: () => void runQueueAction(busyKey, successMessage, action),
                        };
                      }),
                    ].map((row, index) => (
                      <div
                        key={row.key}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 12,
                          padding: '10px 12px',
                          borderTop: index > 0 ? '1px solid var(--border)' : undefined,
                        }}
                      >
                        <div style={{ minWidth: 0 }}>
                          <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
                            <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)' }}>{row.title}</div>
                            <span style={{ fontSize: 12, color: 'var(--accent)', border: '1px solid var(--border)', borderRadius: 999, padding: '2px 7px' }}>
                              {formatQueueStateLabel(row.status)}
                            </span>
                          </div>
                          <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 4 }}>
                            {row.meta}
                          </div>
                        </div>
                        <button
                          type="button"
                          onClick={row.onAction}
                          disabled={queueActionBusy != null}
                          style={{
                            ...hybridControlStyles.iconButton,
                            color: 'var(--danger)',
                            ...(queueActionBusy === row.busyKey ? { opacity: 0.6 } : {}),
                          }}
                          title={queueActionBusy === row.busyKey ? 'Working…' : row.actionTitle}
                          aria-label={row.actionTitle}
                        >
                          {row.actionTitle.startsWith('Stop') ? <QueueIcon.stop /> : <QueueIcon.cancel />}
                        </button>
                      </div>
                    ))}
                  </div>
                  ) : (
                    <div style={{ padding: '10px 12px', borderRadius: 8, border: '1px solid var(--border)', fontSize: 13, color: 'var(--text-muted)' }}>
                      No active scan or post-scan jobs right now.
                    </div>
                  )}
                </div>
                {libraries.length > 0 && (
                  <div>
                    <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>Run maintenance</div>
                    <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 8 }}>
                      These tasks already run automatically after every library scan — use these to trigger one on demand.
                    </div>
                    <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
                      {libraries.map((library, index) => (
                        <div
                          key={`manual-post-scan-${library.id}`}
                          style={{
                            display: 'flex',
                            flexWrap: 'wrap',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: 10,
                            padding: '10px 12px',
                            borderTop: index > 0 ? '1px solid var(--border)' : undefined,
                          }}
                        >
                          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)' }}>{library.name}</div>
                          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
                            {getLibraryPostScanActions(library).map((action) => {
                              const busyKey = `enqueue:${library.id}:${action.jobType}`;
                              const ActionIcon = QueueIcon[action.icon];
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
                                  style={{
                                    ...hybridControlStyles.iconButton,
                                    width: 32,
                                    minWidth: 32,
                                    height: 32,
                                    ...(queueActionBusy === busyKey ? { opacity: 0.6 } : {}),
                                  }}
                                  title={queueActionBusy === busyKey ? 'Queuing…' : action.label}
                                  aria-label={action.label}
                                >
                                  <ActionIcon />
                                </button>
                              );
                            })}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {/* ── Advanced Tab ──────────────────────────────────────────────────── */}
      {activeTab === 'advanced' && (
        <div style={P.section}>
          <div style={hybridSettingsStyles.advancedIntro}>
            <div style={P.sectionTitle}>Advanced settings</div>
            <div style={hybridSettingsStyles.panelDescription}>
              These controls affect playback behavior, background processing, network access, and server maintenance.
              Routine appearance and library choices stay in their dedicated sections.
            </div>
            <nav aria-label="Advanced settings groups" style={hybridSettingsStyles.advancedNav}>
              {[
                ['advanced-playback', 'Playback'],
                ['advanced-transitions', 'Transitions'],
                ['advanced-boogiemix', 'BoogieMix'],
                ['advanced-waveforms', 'Waveforms'],
                ['advanced-bpm', 'BPM'],
                ['advanced-dlna', 'DLNA'],
                ['advanced-debug', 'Diagnostics'],
                ...(isAdmin ? [['advanced-database', 'Database']] : []),
              ].map(([target, label]) => (
                <a key={target} href={`#${target}`} style={hybridSettingsStyles.advancedNavItem}>{label}</a>
              ))}
            </nav>
          </div>

          <div id="advanced-playback" style={P.advancedSectionTitle}>Playback</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <ToggleRow
              title="Server-side transcoding"
              ariaLabel="Server-side transcoding"
              checked={!streamDirect}
              onChange={() => {
                const next = !streamDirect;
                setStreamDirect(next);
                setStreamDirectState(next);
                onStreamDirectChange?.(next);
              }}
              description={(
                <>
                  When enabled, the server converts FLAC, M4A, AAC, WMA and other
                  formats to MP3 before streaming. Disable to stream the raw file
                  bytes directly — useful when your browser natively supports the
                  format (e.g. Safari with FLAC/AAC) or when transcoding causes issues.
                </>
              )}
            />
            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.6 }}>
              <strong style={{ color: 'var(--text)' }}>Note:</strong> This setting is stored as a browser
              cookie and applies only to this device/browser. Other browsers or devices accessing the
              same server will use their own setting. The change takes effect on the next track played.
            </div>
          </div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              Server transcoding quality
            </div>
            <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
              Select MP3 output quality used when server-side transcoding is enabled.
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <select
                value={transcodeQuality}
                onChange={e => { setTranscodeQuality(e.target.value as 'low' | 'high'); setTranscodeQualityResult(null); }}
                style={hybridControlStyles.select}
              >
                <option value="low">Low (192 kbps CBR)</option>
                <option value="high">High (320 kbps CBR)</option>
              </select>
              <button
                type="button"
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
                style={{ ...hybridControlStyles.primaryButton, ...(transcodeQualitySaving ? hybridControlStyles.disabled : {}) }}
              >
                {transcodeQualitySaving ? 'Saving...' : 'Save Quality'}
              </button>
              <InlineStatus busy={false} busyText="" result={transcodeQualityResult} />
            </div>
            <div style={{ marginTop: 14 }}>
              <ToggleRow
                title="ReplayGain normalization"
                ariaLabel="ReplayGain normalization"
                checked={replayGainEnabled}
                onChange={async () => {
                  const next = !replayGainEnabled;
                  setReplayGainEnabled(next);
                  await api.settings.update({ replayGainEnabled: String(next) });
                }}
                description="Normalize loudness across all tracks using EBU R128. Applies during server-side transcoding only."
              />
            </div>
          </div>

          {/* ── Track Transitions ──────────────────────────────────────── */}
          <div id="advanced-transitions" style={P.advancedSectionTitle}>Track Transitions</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 14, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
              Control how tracks transition during playback. Override per-album or per-playlist via their context menus.
            </div>

            {/* Mode picker */}
            <div style={{ marginBottom: 16 }}>
              <label style={{ fontSize: 14, color: 'var(--text-muted)', display: 'block', marginBottom: 6 }}>
                Transition mode
              </label>
              <div style={hybridControlStyles.segmentedGroup}>
                {([
                  { value: 'off', label: 'Off' },
                  { value: 'zerogap', label: 'Zero-gap' },
                  { value: 'crossfade', label: 'Crossfade' },
                ] as const).map(opt => (
                  <button
                    key={opt.value}
                    type="button"
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
                      ...hybridControlStyles.segment,
                      ...(cfMode === opt.value ? hybridControlStyles.segmentActive : {}),
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
                <label style={{ fontSize: 14, color: 'var(--text-muted)', display: 'block', marginBottom: 6 }}>
                  Crossfade duration: <strong style={{ color: 'var(--text)' }}>{cfDuration}s</strong>
                </label>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 13, color: 'var(--text-muted)' }}>1s</span>
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
                  <span style={{ fontSize: 13, color: 'var(--text-muted)' }}>10s</span>
                </div>
              </div>
            )}

            <InlineStatus busy={cfSaving} busyText="Saving…" result={cfResult} />
          </div>

          {/* ── BoogieMix ────────────────────────────────────────────────── */}
          <div id="advanced-boogiemix" style={P.advancedSectionTitle}>BoogieMix</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              BoogieMix output folder
            </div>
            <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
              Admin override for rendered mix files. Leave blank to use the default folder inside the active database directory (`mix-outputs`).
            </div>
            <div style={{ fontSize: 14, color: 'var(--warning)', lineHeight: 1.6, marginBottom: 10, fontWeight: 600 }}>
              BoogieMix is experimental and output quality may vary between runs.
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <input
                type="text"
                value={boogiemixOutputFolder}
                onChange={(e) => { setBoogiemixOutputFolder(e.target.value); setBoogiemixOutputFolderResult(null); }}
                placeholder="Blank = <db-folder>\\mix-outputs"
                style={{ ...hybridControlStyles.field, minWidth: 320, flex: 1, fontFamily: 'monospace' }}
              />
              <button
                type="button"
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
                style={{ ...hybridControlStyles.primaryButton, ...(boogiemixOutputFolderSaving ? hybridControlStyles.disabled : {}) }}
              >
                {boogiemixOutputFolderSaving ? 'Saving...' : 'Save Folder'}
              </button>
              <InlineStatus busy={false} busyText="" result={boogiemixOutputFolderResult} />
            </div>
          </div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              BoogieMix deep analysis
            </div>
            <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
              Torch/Demucs analysis is separate from BPM analysis and is only used by High Quality BoogieMix jobs when the runtime is ready.
            </div>
            {boogiemixDeepLoading ? (
              <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>Loading deep analysis status...</div>
            ) : boogiemixDeepStatus ? (
              <div style={{ display: 'grid', gap: 10, fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.7 }}>
                <div style={{ color: boogiemixDeepStatus.runtime?.enabled ? 'var(--success)' : 'var(--warning)', fontWeight: 600 }}>
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
                    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, background: 'color-mix(in srgb, var(--success) 15%, transparent)', border: '1px solid var(--success)', borderRadius: 10, padding: '1px 8px', color: 'var(--success)', fontWeight: 700, fontSize: 13 }}>
                      <span style={{ width: 7, height: 7, borderRadius: '50%', background: 'var(--success)', display: 'inline-block', animation: 'bm-pulse 1.2s ease-in-out infinite' }} />
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
                      style={{ maxWidth: 360, background: 'var(--bg)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 6, padding: '7px 10px', fontSize: 14 }}
                    >
                      <option value="off">Off</option>
                      <option value="playlists_only">Playlists only</option>
                      <option value="favorites_and_playlists">Favorites and playlists</option>
                      <option value="all_music">All music</option>
                    </select>
                  </label>
                  {boogiemixDeepBackgroundMode === 'all_music' && (
                    <div style={{ color: 'var(--warning)', fontWeight: 600 }}>
                      Full-library deep analysis can take many hours and cause sustained CPU and disk activity.
                    </div>
                  )}
                  {(() => {
                    const totalTracks = libraries.reduce((sum, lib) => sum + (lib.track_count || 0), 0);
                    const analyzedTracks = boogiemixDeepStatus?.cache.analyzedTracks ?? 0;
                    return (
                      <div style={{ display: 'grid', gap: 6, paddingTop: 2 }}>
                        <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>
                          {totalTracks.toLocaleString()} tracks in your collection · {analyzedTracks.toLocaleString()} already analyzed
                        </div>
                        <button
                          disabled={totalTracks === 0 || boogiemixDeepActionBusy === 'reanalyze-all'}
                          onClick={() => setPendingConfirm({
                            title: 'Re-analyze your entire collection?',
                            message: `This queues all ${totalTracks.toLocaleString()} tracks, including the ${analyzedTracks.toLocaleString()} `
                              + 'already analyzed. Nothing is deleted or lost — every one is just re-processed from the audio file.\n\n'
                              + 'Deep analysis takes around a minute per track, so a collection this size can run for days, using '
                              + 'sustained CPU/GPU and disk activity the whole time. It runs through the same background queue as '
                              + 'routine analysis, so Pause Background stops and resumes it.',
                            confirmLabel: 'Force Re-analyze Entire Collection',
                            tone: 'danger',
                            onConfirm: () => runBoogieMixDeepAction('reanalyze-all', async () => {
                              const result = await api.boogiemix.queueAllDeepAnalysis(true);
                              return `Queued ${result.queued} tracks`;
                            }),
                          })}
                          title="Queue every track in every library, not just the ones missing an analysis"
                          style={{
                            ...hybridControlStyles.dangerButton,
                            alignSelf: 'flex-start',
                            ...(totalTracks === 0 ? hybridControlStyles.disabled : {}),
                          }}
                        >
                          Force Re-analyze Entire Collection
                        </button>
                      </div>
                    );
                  })()}
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
                        style={{ width: 80, background: 'var(--bg)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 6, padding: '7px 10px', fontSize: 14 }}
                      />
                      <span style={{ color: 'var(--text-muted)', fontSize: 14 }}>
                        minutes — tracks longer than this are skipped (0 = no limit). Timeout scales automatically at 5 s per track-second.
                      </span>
                    </div>
                  </label>
                  <div style={{ display: 'grid', gap: 6, paddingTop: 2 }}>
                    <span style={{ fontSize: 13, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                      Analysis wait budgets
                    </span>
                    <span style={{ color: 'var(--text-muted)', fontSize: 14 }}>
                      When building a mix, BoogieBox waits up to these limits for deep, BPM, and waveform analysis to
                      finish on the mix's own tracks before continuing anyway (0–60000 ms).
                    </span>
                    {([
                      { key: 'boogiemixHighQualityWaitMs', label: 'Deep analysis (high quality)', value: boogiemixHighQualityWaitMs, setValue: setBoogiemixHighQualityWaitMs, defaultMs: 20000 },
                      { key: 'boogiemixBpmWaitMs', label: 'BPM analysis', value: boogiemixBpmWaitMs, setValue: setBoogiemixBpmWaitMs, defaultMs: 15000 },
                      { key: 'boogiemixWaveformWaitMs', label: 'Waveform analysis', value: boogiemixWaveformWaitMs, setValue: setBoogiemixWaveformWaitMs, defaultMs: 15000 },
                    ] as const).map(({ key, label, value, setValue, defaultMs }) => (
                      <label key={key} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span style={{ color: 'var(--text)', minWidth: 200 }}>{label}</span>
                        <input
                          type="number"
                          min={0}
                          max={60000}
                          step={500}
                          value={value}
                          onChange={(e) => setValue(Math.min(60000, Math.max(0, Number(e.target.value) || 0)))}
                          onBlur={() => {
                            const saveValue = String(value);
                            runBoogieMixDeepAction(`${key}-wait`, async () => {
                              await api.settings.update({ [key]: saveValue });
                              return `${label} wait saved`;
                            });
                          }}
                          style={{ width: 90, background: 'var(--bg)', color: 'var(--text)', border: '1px solid var(--border)', borderRadius: 6, padding: '7px 10px', fontSize: 14 }}
                        />
                        <span style={{ color: 'var(--text-muted)', fontSize: 13 }}>ms (default {defaultMs.toLocaleString()})</span>
                      </label>
                    ))}
                  </div>
                  {/* Analysis model selector */}
                  {(() => {
                    const gpuAvailable = boogiemixDeepStatus?.runtime?.gpuAvailable ?? false;
                    const effectiveModel = gpuAvailable ? boogiemixDeepModel : 'hpss';
                    const models: { value: string; label: string; desc: string }[] = [
                      { value: 'mdx_extra_q', label: 'Full (mdx_extra_q)', desc: 'Best accuracy, GPU recommended' },
                      { value: 'htdemucs', label: 'Light (htdemucs)', desc: 'Faster, still accurate, GPU recommended' },
                      { value: 'hpss', label: 'Ultralight (HPSS)', desc: 'Seconds per track, CPU-native, less precise stems' },
                    ];
                    return (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <span style={{ fontSize: 13, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Analysis model</span>
                        <div style={hybridControlStyles.segmentedGroup}>
                          {models.map(({ value, label, desc }) => {
                            const forced = !gpuAvailable;
                            const active = effectiveModel === value;
                            const disabled = forced && value !== 'hpss';
                            return (
                              <button
                                key={value}
                                type="button"
                                disabled={disabled}
                                title={desc}
                                onClick={() => {
                                  if (disabled || forced) return;
                                  setBoogiemixDeepModel(value);
                                  runBoogieMixDeepAction('model-select', async () => {
                                    await api.settings.update({ boogiemixDeepAnalysisModel: value });
                                    return `Model set to ${label}`;
                                  });
                                }}
                                style={{
                                  ...hybridControlStyles.segment,
                                  ...(active ? hybridControlStyles.segmentActive : {}),
                                  ...(disabled ? hybridControlStyles.disabled : {}),
                                }}
                              >
                                {label}
                              </button>
                            );
                          })}
                        </div>
                        {!gpuAvailable && (
                          <span style={{ fontSize: 13, color: 'var(--text-muted)' }}>HPSS forced — no GPU detected on the server</span>
                        )}
                      </div>
                    );
                  })()}
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
                    <select
                      value={boogiemixDeepSelectedLibrary}
                      onChange={(e) => setBoogiemixDeepSelectedLibrary(e.target.value as ClientEntityId)}
                      aria-label="BoogieMix deep-analysis library"
                      style={{ ...hybridControlStyles.select, minWidth: 220 }}
                    >
                      <option value="">Select library</option>
                      {libraries.map((library) => (
                        <option key={String(library.id)} value={String(library.id)}>{library.name}</option>
                      ))}
                    </select>
                    <button
                      type="button"
                      disabled={!boogiemixDeepSelectedLibrary || boogiemixDeepActionBusy === 'queue-library'}
                      onClick={() => runBoogieMixDeepAction('queue-library', async () => {
                        const result = await api.boogiemix.queueLibraryDeepAnalysis(boogiemixDeepSelectedLibrary);
                        return `Queued ${result.queued} tracks`;
                      })}
                      style={{ ...hybridControlStyles.secondaryButton, ...(!boogiemixDeepSelectedLibrary ? hybridControlStyles.disabled : {}) }}
                    >
                      Analyze Library
                    </button>
                    <button
                      type="button"
                      disabled={!boogiemixDeepSelectedLibrary || boogiemixDeepActionBusy === 'reanalyze-library'}
                      onClick={() => setPendingConfirm({
                        title: 'Re-analyze every track in this library?',
                        message: 'Including ones already analyzed.\n\n'
                          + 'This discards nothing, but deep analysis takes around a minute per track, '
                          + 'so a large library can run for many hours.',
                        confirmLabel: 'Re-analyze All',
                        onConfirm: () => runBoogieMixDeepAction('reanalyze-library', async () => {
                          const result = await api.boogiemix.queueLibraryDeepAnalysis(boogiemixDeepSelectedLibrary, true);
                          return `Queued ${result.queued} tracks`;
                        }),
                      })}
                      title="Queue every track, not just the ones missing an analysis"
                      style={{ ...hybridControlStyles.secondaryButton, ...(!boogiemixDeepSelectedLibrary ? hybridControlStyles.disabled : {}) }}
                    >
                      Re-analyze All
                    </button>
                    <button
                      type="button"
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
                      style={hybridControlStyles.secondaryButton}
                    >
                      {boogiemixDeepPauseBackground ? 'Resume Background' : 'Pause Background'}
                    </button>
                    <button
                      type="button"
                      onClick={() => setPendingConfirm({
                        title: 'Clear stored BoogieMix deep-analysis cache?',
                        message: 'Every track will need to be re-analyzed before its next BoogieMix.',
                        confirmLabel: 'Clear Cache',
                        tone: 'danger',
                        onConfirm: () => runBoogieMixDeepAction('clear-cache', async () => {
                          const result = await api.boogiemix.clearDeepAnalysisCache();
                          return `Cleared ${result.deletedCacheRows} cached rows`;
                        }),
                      })}
                      disabled={boogiemixDeepActionBusy === 'clear-cache'}
                      style={hybridControlStyles.dangerButton}
                    >
                      Clear Cache
                    </button>
                  </div>
                  <InlineStatus busy={false} busyText="" result={boogiemixDeepActionResult} />
                </div>
              </div>
            ) : (
              <div style={{ fontSize: 13, color: 'var(--warning)' }}>
                Deep analysis status unavailable. High Quality mixes will report fallback details when created.
              </div>
            )}
          </div>

          {/* ── Waveforms ───────────────────────────────────────────────── */}
          <div id="advanced-waveforms" style={P.advancedSectionTitle}>Waveforms</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <ToggleRow
              title="Generate waveform when missing"
              ariaLabel="Generate waveform when missing"
              checked={waveformGenerateOnMissing}
              onChange={async () => {
                const next = !waveformGenerateOnMissing;
                setWaveformGenerateOnMissing(next);
                await saveWaveformSettings({ waveformGenerateOnMissing: next ? 'true' : 'false' });
              }}
              description="If enabled, BoogieBox starts waveform generation on playback for tracks that do not already have cached waveform data."
            />

            <ToggleRow
              title="Enable waveform background mapping"
              ariaLabel="Enable waveform background mapping"
              checked={waveformBackgroundEnabled}
              onChange={async () => {
                const next = !waveformBackgroundEnabled;
                setWaveformBackgroundEnabled(next);
                await saveWaveformSettings({ waveformBackgroundEnabled: next ? 'true' : 'false' });
              }}
              description="Runs a scheduled background task that maps waveform data for tracks that are still missing it."
            />

            <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap', marginBottom: 16 }}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <label style={{ fontSize: 13, color: 'var(--text-muted)' }}>Mapping frequency</label>
                <select
                  value={waveformFrequencyHours}
                  onChange={async (e) => {
                    const next = Number(e.target.value);
                    setWaveformFrequencyHours(next);
                    await saveWaveformSettings({ waveformBackgroundFrequencyHours: String(next) });
                  }}
                  disabled={!waveformBackgroundEnabled}
                  style={{ ...hybridControlStyles.select, ...(waveformBackgroundEnabled ? {} : hybridControlStyles.disabled) }}
                >
                  {FREQ_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </select>
              </div>

              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <label style={{ fontSize: 13, color: 'var(--text-muted)' }}>Batch size per run</label>
                <select
                  value={waveformBatchSize}
                  onChange={async (e) => {
                    const next = Number(e.target.value);
                    setWaveformBatchSize(next);
                    await saveWaveformSettings({ waveformBackgroundBatchSize: String(next) });
                  }}
                  style={hybridControlStyles.select}
                >
                  {WAVEFORM_BATCH_OPTIONS.map((opt) => (
                    <option key={opt} value={opt}>{opt} tracks</option>
                  ))}
                </select>
              </div>

              <button type="button" onClick={runWaveformMappingNow} style={{ ...hybridControlStyles.secondaryButton, marginTop: 18 }}>
                Run Mapping Now
              </button>
            </div>

            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.7 }}>
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
              <div style={{ fontSize: 13, marginTop: 10, color: 'var(--text-muted)' }}>
                {waveformSaving ? 'Saving waveform settings...' : waveformSettingsResult}
                {waveformRunResult ? <div style={{ marginTop: 4 }}>{waveformRunResult}</div> : null}
              </div>
            )}
          </div>

          {/* ── BPM Analysis ────────────────────────────────────────────── */}
          <div id="advanced-bpm" style={P.advancedSectionTitle}>BPM Analysis</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <ToggleRow
              title="Enable BPM background analysis"
              ariaLabel="Enable BPM background analysis"
              checked={bpmBackgroundEnabled}
              onChange={async () => {
                const next = !bpmBackgroundEnabled;
                setBpmBackgroundEnabled(next);
                await saveBpmSettings({ bpmBackgroundEnabled: next ? 'true' : 'false' });
              }}
              description="Runs BPM analysis on a schedule for tracks that are still missing BPM data."
            />

            <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap', marginBottom: 12 }}>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <label style={{ fontSize: 13, color: 'var(--text-muted)' }}>Analysis frequency</label>
                <select
                  value={bpmFrequencyHours}
                  onChange={async (e) => {
                    const next = Number(e.target.value);
                    setBpmFrequencyHours(next);
                    await saveBpmSettings({ bpmBackgroundFrequencyHours: String(next) });
                  }}
                  disabled={!bpmBackgroundEnabled}
                  style={{ ...hybridControlStyles.select, ...(bpmBackgroundEnabled ? {} : hybridControlStyles.disabled) }}
                >
                  {FREQ_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </select>
              </div>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 12 }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  BPM Detection
                </div>
                <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                  Detect BPM for tracks using local FFmpeg audio analysis.
                  Results are used by BoogieMix for better tempo matching and transitions.
                </div>
              </div>
              <button
                type="button"
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
                style={{ ...hybridControlStyles.secondaryButton, whiteSpace: 'nowrap' }}
              >
                Run BPM Analysis
              </button>
            </div>

            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.7 }}>
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
              <div style={{ fontSize: 13, marginTop: 10, color: 'var(--text-muted)' }}>
                {bpmSaving ? 'Saving BPM settings...' : bpmSettingsResult}
                {bpmRunResult ? <div style={{ marginTop: 4 }}>{bpmRunResult}</div> : null}
              </div>
            )}
          </div>

          {/* ── DLNA Server ─────────────────────────────────────────────── */}
          <div id="advanced-dlna" style={P.advancedSectionTitle}>DLNA Server</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            <ToggleRow
              title="Enable DLNA server"
              ariaLabel="Enable DLNA server"
              checked={dlnaEnabled}
              onChange={() => setDlnaEnabled(!dlnaEnabled)}
              description="Broadcast your music libraries on the local network so DLNA-compatible devices can discover and stream audio."
            />

            {/* Friendly name */}
            <div style={{ marginBottom: 12 }}>
              <label style={{ fontSize: 14, color: 'var(--text-muted)', display: 'block', marginBottom: 4 }}>
                Device name (shown on network)
              </label>
              <input
                type="text"
                value={dlnaFriendlyName}
                onChange={e => setDlnaFriendlyName(e.target.value)}
                placeholder="BoogieBox"
                style={{
                  width: '100%', maxWidth: 280, background: 'var(--bg)', border: '1px solid var(--border)',
                  color: 'var(--text)', borderRadius: 6, padding: '7px 10px', fontSize: 14,
                  fontFamily: 'monospace', outline: 'none', boxSizing: 'border-box',
                }}
              />
            </div>

            {/* Port */}
            <div style={{ marginBottom: 16 }}>
              <label style={{ fontSize: 14, color: 'var(--text-muted)', display: 'block', marginBottom: 4 }}>
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
                  color: 'var(--text)', borderRadius: 6, padding: '7px 10px', fontSize: 14,
                  fontFamily: 'monospace', outline: 'none',
                }}
              />
            </div>

            {/* Status indicator */}
            {dlnaStatus && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
                <div style={{
                  width: 8, height: 8, borderRadius: '50%',
                  backgroundColor: dlnaStatus.running ? 'var(--success)' : 'var(--border)',
                }} />
                <span style={{ fontSize: 14, color: 'var(--text-muted)' }}>
                  {dlnaStatus.running
                    ? `Running on port ${dlnaStatus.port} as "${dlnaStatus.friendlyName}"`
                    : 'Stopped'}
                </span>
              </div>
            )}

            {/* Save button */}
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <button
                type="button"
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
                style={{ ...hybridControlStyles.primaryButton, ...(dlnaSaving ? hybridControlStyles.disabled : {}) }}
              >
                {dlnaSaving ? 'Saving…' : 'Save DLNA Settings'}
              </button>
              <InlineStatus busy={false} busyText="" result={dlnaResult} />
            </div>
          </div>

          {/* ── Debug Logging ───────────────────────────────────────────── */}
          <div id="advanced-debug" style={P.advancedSectionTitle}>Debug Logging</div>

          <div style={{
            padding: '16px 20px', borderRadius: 8, marginBottom: 12,
            backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
          }}>
            {/* Scan debug */}
            <ToggleRow
              title="Scan debug logging"
              ariaLabel="Scan debug logging"
              checked={scanDebugLoggingEnabled}
              disabled={scanDebugSaving}
              onChange={async () => {
                if (scanDebugSaving) return;
                const next = !scanDebugLoggingEnabled;
                setScanDebugLoggingEnabled(next);
                await saveScanDebugSettings(next);
              }}
              description={(
                <>
                  Writes detailed scan and post-scan diagnostics — including metadata provider requests
                  (Discogs, Last.fm, Deezer, Spotify, LRCLIB, lyrics.ovh) and their results — to{' '}
                  <code style={{ color: 'var(--accent)' }}>scan-debug.log</code>. Takes effect immediately, no restart
                  needed. Leave this disabled unless you are troubleshooting large-library scans or stuck post-scan work.
                </>
              )}
            />
            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.7, marginBottom: 4 }}>
              Debug mode is <strong style={{ color: 'var(--text)' }}>{scanDebugLoggingEnabled ? 'enabled' : 'disabled'}</strong>.
              When enabled, scan queueing, file and batch checkpoints, follow-up job dispatch, worker progress, failures, and completion events are appended to the shared debug log.
            </div>
            <InlineStatus busy={scanDebugSaving} busyText="Saving…" result={scanDebugResult} />

            <div style={{ height: 1, backgroundColor: 'var(--border)', margin: '16px 0' }} />

            {/* Deep analysis debug */}
            <ToggleRow
              title="BoogieMix deep analysis debug logging"
              ariaLabel="BoogieMix deep analysis debug logging"
              checked={deepmixDebugLoggingEnabled}
              disabled={deepmixDebugSaving}
              onChange={async () => {
                if (deepmixDebugSaving) return;
                const next = !deepmixDebugLoggingEnabled;
                setDeepmixDebugLoggingEnabled(next);
                await saveDeepmixDebugSettings(next);
              }}
              description={(
                <>
                  Writes verbose deep analysis diagnostics to <code style={{ color: 'var(--accent)' }}>deep-analysis-debug.log</code>.
                  Logs every tick decision, Python runtime detection step (candidates, venv paths, version checks,
                  demucs/torch/CUDA probes), job claim, worker spawn, stdin write, process exit status, stdout/stderr,
                  JSON parse result, DB upsert outcome, and completion. Takes effect immediately, no restart needed.
                  Enable when deep analysis jobs are timing out or producing no output.
                </>
              )}
            />
            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.7, marginBottom: 4 }}>
              Deep analysis debug is <strong style={{ color: 'var(--text)' }}>{deepmixDebugLoggingEnabled ? 'enabled' : 'disabled'}</strong>.
              Logs appear in <code style={{ color: 'var(--accent)' }}>deep-analysis-debug.log</code> under the <code style={{ color: 'var(--accent)'}}>[boogiemix:deep]</code> prefix.
              Disable after investigation — the server's main log stays clean regardless.
            </div>
            <InlineStatus busy={deepmixDebugSaving} busyText="Saving…" result={deepmixDebugResult} />

            {(logFilePaths.server || logFilePaths.scan || logFilePaths.deep) && (
              <>
                <div style={{ height: 1, backgroundColor: 'var(--border)', margin: '16px 0' }} />
                <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>
                  Log files
                </div>
                <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.9, fontFamily: 'monospace', wordBreak: 'break-all' }}>
                  {logFilePaths.server && <div>server.log — {logFilePaths.server}</div>}
                  {logFilePaths.scan && <div>scan-debug.log — {logFilePaths.scan}</div>}
                  {logFilePaths.deep && <div>deep-analysis-debug.log — {logFilePaths.deep}</div>}
                </div>
              </>
            )}
          </div>

          {/* ── Database (admin only) ─────────────────────────────────────── */}
          {isAdmin && (
            <>
              <div id="advanced-database" style={P.advancedSectionTitle}>Database</div>
              <div style={{
                padding: '16px 20px', borderRadius: 8, marginBottom: 12,
                backgroundColor: 'var(--surface)', border: '1px solid var(--border)',
              }}>
                <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                  Active database folder
                </div>
                {currentDbFolder && (
                  <div style={{ fontSize: 14, color: 'var(--text-muted)', marginBottom: 12, fontFamily: 'monospace', wordBreak: 'break-all' }}>
                    {currentDbFolder}
                  </div>
                )}
                <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 10 }}>
                  Switch to a different database folder. The server will reload all settings from the new database.
                  If the folder does not contain a database yet, a fresh one will be created.
                </div>
                <div style={{ fontSize: 14, color: 'var(--warning)', lineHeight: 1.6, marginBottom: 10, fontWeight: 600 }}>
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
                      fontSize: 14, fontFamily: 'inherit', outline: 'none',
                    }}
                  />
                  <button
                    type="button"
                    onClick={() => setShowDbFolderPicker(true)}
                    style={hybridControlStyles.secondaryButton}
                  >
                    Browse...
                  </button>
                  <button
                    type="button"
                    disabled={switchDbSaving || !switchDbFolder.trim()}
                    onClick={() => setPendingConfirm({
                      title: 'Switch to a different database?',
                      message: `${switchDbFolder.trim()}\n\nThe page will reload after switching.`,
                      confirmLabel: 'Switch Database',
                      onConfirm: async () => {
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
                      },
                    })}
                    style={{
                      ...hybridControlStyles.dangerButton,
                      ...((switchDbSaving || !switchDbFolder.trim()) ? hybridControlStyles.disabled : {}),
                    }}
                  >
                    {switchDbSaving ? 'Switching...' : 'Switch Database'}
                  </button>
                </div>
                <InlineStatus busy={false} busyText="" result={switchDbResult} />
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
          <IntegrationConnector
            dotColor="#333"
            name="Discogs"
            description="Fetches album cover art when no local folder.jpg is found. Local images are always preferred and served directly from your server."
            status={connectorStatus(!!discogsToken.trim(), discogsConnected)}
            helpOpen={discogsHelpOpen}
            onToggleHelp={() => setDiscogsHelpOpen((v) => !v)}
            helpContent={
              <>
                <strong style={{ color: 'var(--text)' }}>1.</strong> Sign in at{' '}
                <a href="https://www.discogs.com/settings/developers" target="_blank" rel="noreferrer" style={{ color: 'var(--accent)' }}>
                  discogs.com/settings/developers
                </a>{' '}
                <strong style={{ color: 'var(--text)' }}>2.</strong> Click <em>Generate new token</em>{' '}
                <strong style={{ color: 'var(--text)' }}>3.</strong> Paste it below.
                <div style={{ marginTop: 8 }}>
                  When you open an album, BoogieBox checks the album's folder for a local image
                  (<code>folder.jpg</code>, <code>cover.jpg</code>, <code>front.jpg</code>, etc.).
                  If none is found and a Discogs token is configured, it searches Discogs and
                  displays the cover from there — no data is sent to Discogs for local images.
                </div>
              </>
            }
            onTest={testDiscogsToken}
            onSave={saveDiscogsToken}
            busy={discogsSaving}
            errorMessage={connectorMessage(discogsTestResult)}
          >
            <input
              type="password"
              placeholder="Paste your Discogs personal access token…"
              value={discogsToken}
              onChange={e => { setDiscogsToken(e.target.value); setDiscogsTestResult(null); setDiscogsConnected(null); }}
              style={{
                flex: 1, minWidth: 240,
                backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                fontSize: 15, fontFamily: 'monospace', outline: 'none',
              }}
            />
          </IntegrationConnector>

          {showGeniusIntegration && <div style={{ ...P.sectionTitle, marginTop: 32 }}>Lyrics: Genius</div>}
          {showGeniusIntegration && <p style={{ color: 'var(--text-muted)', fontSize: 14, marginBottom: 20, lineHeight: 1.7 }}>
            Used to retrieve lyrics for tracks.
          </p>}

          {showGeniusIntegration && <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
            <div style={{ fontSize: 14, color: 'var(--text-muted)', marginBottom: 12, lineHeight: 1.6 }}>
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
                  fontSize: 15, fontFamily: 'monospace', outline: 'none',
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
                  fontSize: 15, fontFamily: 'monospace', outline: 'none',
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
                marginTop: 10, padding: '8px 12px', borderRadius: 6, fontSize: 14,
                backgroundColor: geniusResult.startsWith('OK') ? 'rgba(34,197,94,0.08)' : 'rgba(239,68,68,0.08)',
                border: `1px solid ${geniusResult.startsWith('OK') ? 'rgba(34,197,94,0.3)' : 'rgba(239,68,68,0.3)'}`,
                color: geniusResult.startsWith('OK') ? '#86efac' : '#fca5a5',
              }}>
                {geniusResult}
              </div>
            )}
          </div>}

          {/* Last.fm */}
          <div style={{ ...P.sectionTitle, marginTop: 24 }}>Last.fm</div>
          <IntegrationConnector
            dotColor="#d51007"
            name="Last.fm"
            description="Shows artist biographies, album reviews, listener stats, and genre tags on artist and album pages in Browse."
            status={connectorStatus(!!lastfmKey.trim(), lastfmConnected)}
            helpOpen={lastfmHelpOpen}
            onToggleHelp={() => setLastfmHelpOpen((v) => !v)}
            helpContent={
              <>
                <strong style={{ color: 'var(--text)' }}>1.</strong> Create an account at{' '}
                <a href="https://www.last.fm/api/account/create" target="_blank" rel="noreferrer" style={{ color: 'var(--accent)' }}>
                  last.fm/api/account/create
                </a>{' '}
                <strong style={{ color: 'var(--text)' }}>2.</strong> Fill in the application form{' '}
                <strong style={{ color: 'var(--text)' }}>3.</strong> Copy your API key and paste it below.
              </>
            }
            onTest={testLastfmKey}
            onSave={saveLastfmKey}
            busy={lastfmSaving}
            errorMessage={connectorMessage(lastfmResult)}
          >
            <input
              type="password"
              placeholder="Paste your Last.fm API key…"
              value={lastfmKey}
              onChange={e => { setLastfmKey(e.target.value); setLastfmResult(null); setLastfmConnected(null); }}
              style={{
                flex: 1, minWidth: 240,
                backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                fontSize: 15, fontFamily: 'monospace', outline: 'none',
              }}
            />
          </IntegrationConnector>

          <div style={{ ...P.sectionTitle, marginTop: 24 }}>Artist Images: Deezer + Spotify Fallback</div>
          <IntegrationConnector
            dotColor="#1db954"
            name="Spotify"
            description="Artist photos use Deezer first when no local image is available, Discogs as secondary fallback, and Spotify as the final fallback."
            status={connectorStatus(hasSpotifyCredentials(spotifyClientId, spotifyClientSecret), spotifyConnected)}
            helpOpen={spotifyHelpOpen}
            onToggleHelp={() => setSpotifyHelpOpen((v) => !v)}
            helpContent={
              <>
                Create an app at{' '}
                <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noreferrer" style={{ color: 'var(--accent)' }}>
                  developer.spotify.com/dashboard
                </a>{' '}and copy the Client ID and Client Secret.
              </>
            }
            onTest={testSpotifyCreds}
            onSave={saveSpotifyCreds}
            busy={spotifySaving}
            errorMessage={connectorMessage(spotifyResult)}
          >
            <input
              type="text"
              placeholder="Spotify Client ID..."
              value={spotifyClientId}
              onChange={e => { setSpotifyClientId(e.target.value); setSpotifyResult(null); setSpotifyConnected(null); }}
              style={{
                flex: 1, minWidth: 220,
                backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                fontSize: 15, fontFamily: 'monospace', outline: 'none',
              }}
            />
            <input
              type="password"
              placeholder="Spotify Client Secret..."
              value={spotifyClientSecret}
              onChange={e => { setSpotifyClientSecret(e.target.value); setSpotifyResult(null); setSpotifyConnected(null); }}
              style={{
                flex: 1, minWidth: 220,
                backgroundColor: 'var(--bg)', border: '1px solid var(--border)',
                color: 'var(--text)', borderRadius: 6, padding: '8px 12px',
                fontSize: 15, fontFamily: 'monospace', outline: 'none',
              }}
            />
          </IntegrationConnector>

          {isAdmin && (
            <>
              <div style={{ ...P.sectionTitle, marginTop: 32 }}>Provider Usage</div>
              <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)', marginBottom: 20 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'flex-start', flexWrap: 'wrap', marginBottom: 14 }}>
                  <div>
                    <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
                      Metadata provider usage
                    </div>
                    <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 }}>
                      Counts increase only when BoogieBox actually caches provider-backed data or returns provider-backed data to the UI.
                    </div>
                    <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 6 }}>
                      Snapshot: {providerUsage ? fmtQueueTime(providerUsage.fetched_at) : 'Not loaded'}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => loadProviderUsage()}
                    disabled={providerUsageLoading}
                    style={{ ...hybridControlStyles.iconButton, ...(providerUsageLoading ? { opacity: 0.6 } : {}) }}
                    title={providerUsageLoading ? 'Refreshing…' : 'Refresh usage'}
                    aria-label={providerUsageLoading ? 'Refreshing usage' : 'Refresh provider usage'}
                  >
                    <QueueIcon.refresh />
                  </button>
                </div>

                {providerUsageError && (
                  <div style={{ fontSize: 14, color: '#ef4444', marginBottom: 12 }}>{providerUsageError}</div>
                )}

                {!providerUsageLoading && providerUsage && providerUsage.providers.length === 0 && (
                  <div style={{ fontSize: 14, color: 'var(--text-muted)' }}>No provider usage has been recorded yet.</div>
                )}

                {!!providerUsage?.providers.length && (
                  <div style={{ display: 'grid', gap: 10, gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))' }}>
                    {providerUsage.providers.map((provider: ProviderUsageProviderSummary) => (
                      <div key={provider.provider} style={{ border: '1px solid var(--border)', borderRadius: 8, padding: '12px 14px', backgroundColor: 'color-mix(in srgb, var(--surface) 88%, var(--bg))' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8, alignItems: 'baseline' }}>
                          <div style={{ fontSize: 15, fontWeight: 700, color: 'var(--text)' }}>{formatProviderLabel(provider.provider)}</div>
                          <div style={{ fontSize: 20, fontWeight: 700, color: 'var(--accent)', fontVariantNumeric: 'tabular-nums' }}>{provider.total_count}</div>
                        </div>
                        <div style={{ fontSize: 13, color: 'var(--text-muted)', marginTop: 8, lineHeight: 1.6 }}>
                          {Object.entries(provider.usage_breakdown)
                            .sort((a, b) => b[1] - a[1])
                            .map(([usageType, count]) => `${usageType.replace(/_/g, ' ')}: ${count}`)
                            .join(' • ')}
                        </div>
                        <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 8 }}>
                          Last used: {provider.last_used_at ? fmtQueueTime(provider.last_used_at) : 'Never'}
                        </div>
                      </div>
                    ))}
                  </div>
                )}

                {!!providerUsage?.rows.length && (
                  <div style={{ marginTop: 14 }}>
                    <button
                      type="button"
                      onClick={() => setShowProviderUsageRows((value) => !value)}
                      style={{
                        ...hybridControlStyles.iconButton,
                        ...(showProviderUsageRows ? { color: 'var(--accent)', borderColor: 'var(--accent)' } : {}),
                      }}
                      title={showProviderUsageRows ? 'Hide usage rows' : 'Show usage rows'}
                      aria-label={showProviderUsageRows ? 'Hide usage rows' : 'Show usage rows'}
                    >
                      <QueueIcon.code />
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
                              fontSize: 13,
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

      {pendingConfirm && (
        <ConfirmModal
          title={pendingConfirm.title}
          message={pendingConfirm.message}
          confirmLabel={pendingConfirm.confirmLabel}
          tone={pendingConfirm.tone}
          onConfirm={() => { const { onConfirm } = pendingConfirm; setPendingConfirm(null); onConfirm(); }}
          onCancel={() => setPendingConfirm(null)}
        />
      )}
    </div>
  );
}

// ─── Styles ───────────────────────────────────────────────────────────────────

const P: Record<string, React.CSSProperties> = {
  page: {
    flex: 1,
    overflowY: 'auto',
    ...hybridSettingsStyles.page,
  },
  pageHeader: hybridSettingsStyles.pageHeader,
  title: hybridSettingsStyles.title,
  subtitle: hybridSettingsStyles.subtitle,
  account: hybridSettingsStyles.account,
  roleBadge: hybridSettingsStyles.roleBadge,
  logoutButton: { ...hybridControlStyles.secondaryButton, minHeight: 32, padding: '6px 10px' },
  tabBar: hybridSettingsStyles.tabBar,
  tab: hybridSettingsStyles.tab,
  tabActive: hybridSettingsStyles.tabActive,
  section: hybridSettingsStyles.section,
  sectionTitle: hybridSettingsStyles.sectionTitle,
  advancedSectionTitle: {
    ...hybridSettingsStyles.sectionTitle,
    marginTop: 28,
    scrollMarginTop: 16,
  },
  btnPrimary: hybridControlStyles.primaryButton,
  btnSecondary: hybridControlStyles.secondaryButton,
  aboutPanel: {
    padding: '20px 24px',
    borderRadius: 14,
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--divider-subtle)',
  },
  aboutTitle: {
    margin: '0 0 10px',
    fontSize: 20,
    fontWeight: 700,
    color: 'var(--text)',
  },
  aboutCopy: {
    margin: '0 0 18px',
    maxWidth: 620,
    color: 'var(--text-muted)',
    fontSize: 15,
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
