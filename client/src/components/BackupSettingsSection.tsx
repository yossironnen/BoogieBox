/**
 * Settings › Advanced › Backup (admin): status, "Back up now", schedule, folder, extras, the list of backups and
 * restoring one.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import type { BackupSchedule, BackupSettings, BackupsOverview, BackupSummary, PathRemap } from '../types';
import { beginRestoreHere, cancelRestoreHere, finishRestoreHere } from '../restoreState';
import { hybridControlStyles } from '../hybridPreview';
import ConfirmModal from './ConfirmModal';
import FolderPickerModal from './FolderPickerModal';
import RestoreBackupDialog from './RestoreBackupDialog';
import {
  BACKUP_REASON_LABELS,
  BackupIcons as I,
  SCHEDULE_DAYS,
  formatBackupDate,
  formatBytes,
  formatNextRun,
  formatRecentBackupDate,
} from './backupUi';

const POLL_MS = 1500;

type RestoreTarget = { summary: BackupSummary; source: { name: string } | { path: string } };

const SCHEDULE_OPTIONS: Array<{ value: BackupSchedule; label: string; icon: React.ReactNode }> = [
  { value: 'off', label: 'Off', icon: <I.Off size={14} /> },
  { value: 'daily', label: 'Daily', icon: <I.Sun size={14} /> },
  { value: 'weekly', label: 'Weekly', icon: <I.Calendar size={14} /> },
];

function isRegular(b: BackupSummary) {
  return b.reason === 'scheduled' || b.reason === 'manual';
}

export default function BackupSettingsSection({ titleStyle }: { titleStyle?: React.CSSProperties }) {
  const [overview, setOverview] = useState<BackupsOverview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [draft, setDraft] = useState<BackupSettings | null>(null);
  const [scheduleSaved, setScheduleSaved] = useState<string | null>(null);
  const [folderSaved, setFolderSaved] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [picker, setPicker] = useState<'folder' | 'restore-from' | null>(null);
  const [restore, setRestore] = useState<RestoreTarget | null>(null);
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<BackupSummary | null>(null);

  const apply = useCallback((next: BackupsOverview, resetDraft = false) => {
    setOverview(next);
    setDraft((current) => (resetDraft || !current ? next.settings : current));
  }, []);

  const load = useCallback(async (resetDraft = false) => {
    try {
      apply(await api.backups.overview(), resetDraft);
      setLoadError(null);
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    }
  }, [apply]);

  useEffect(() => { void load(true); }, [load]);

  const running = overview?.running ?? null;
  useEffect(() => {
    if (!running) return undefined;
    const id = window.setInterval(() => { void load(); }, POLL_MS);
    return () => window.clearInterval(id);
  }, [load, running]);

  const save = async (settings: BackupSettings, done: (message: string) => void) => {
    try {
      apply(await api.backups.saveSettings(settings), true);
      done('Saved');
    } catch (e) {
      done(e instanceof Error ? e.message : String(e));
    }
  };

  const backUpNow = async () => {
    setNotice(null);
    try {
      await api.backups.create();
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    await load();
  };

  const openRestoreFrom = async (path: string) => {
    setPicker(null);
    setNotice(null);
    try {
      const summary = await api.backups.inspect(path);
      setRestoreError(null);
      setRestore({ summary, source: { path } });
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  };

  const confirmRestore = async (remaps: PathRemap[]) => {
    if (!restore) return;
    setRestoreBusy(true);
    setRestoreError(null);
    beginRestoreHere();
    try {
      const result = 'name' in restore.source
        ? await api.backups.restore(restore.source.name, remaps)
        : await api.backups.restoreFrom(restore.source.path, remaps);
      if (result.ok) {
        finishRestoreHere(restore.summary.createdAt);
        return;
      }
      cancelRestoreHere();
      setRestoreError(result.error ?? 'Restore failed');
    } catch (e) {
      cancelRestoreHere();
      setRestoreError(e instanceof Error ? e.message : String(e));
    } finally {
      setRestoreBusy(false);
    }
  };

  const confirmDelete = async () => {
    if (!deleting) return;
    const target = deleting;
    setDeleting(null);
    try {
      await api.backups.remove(target.name);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    await load();
  };

  const backups = overview?.backups ?? [];
  const regular = backups.filter(isRegular);
  const newest = regular[0] ?? null;
  const settings = overview?.settings ?? null;
  const failure = overview?.lastFailure ?? null;
  const lastGood = newest;

  return (
    <>
      <div id="advanced-backup" style={titleStyle}>Backup</div>

      {loadError && <div role="alert" style={{ ...S.card, color: 'var(--danger)' }}>{loadError}</div>}

      {failure && (
        <div role="alert" style={S.failure}>
          <I.Warning size={20} color="var(--danger, #f07b76)" style={{ flexShrink: 0, marginTop: 1 }} />
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            <div style={{ fontSize: 14, fontWeight: 700 }}>The last backup failed</div>
            <div style={S.muted}>
              {failure.message}
              {lastGood ? ` Your last good backup is from ${formatBackupDate(lastGood.createdAt)}.` : ''}
            </div>
          </div>
        </div>
      )}

      <div style={S.tiles}>
        <div style={S.tile}>
          <I.LastBackup size={28} color="var(--accent)" style={S.tileIcon} />
          <div style={S.tileText}>
            <div style={S.tileLabel}>Last backup</div>
            <div style={S.tileValue}>{running?.kind === 'backup' ? 'Backing up…' : newest ? formatRecentBackupDate(newest.createdAt) : '—'}</div>
            {newest && <div style={S.tileSub}>{formatBytes(newest.sizeBytes)}</div>}
          </div>
        </div>
        <div style={S.tile}>
          <I.Calendar size={28} color="var(--accent)" style={S.tileIcon} />
          <div style={S.tileText}>
            <div style={S.tileLabel}>Next backup</div>
            <div style={S.tileValue}>{settings?.schedule === 'off' || !overview?.nextRun ? 'Off' : formatNextRun(overview.nextRun)}</div>
            {settings && settings.schedule !== 'off' && (
              <div style={S.tileSub}>{settings.schedule === 'daily' ? 'Daily' : 'Weekly'}</div>
            )}
          </div>
        </div>
        <div style={S.tile}>
          <I.Archive size={28} color="var(--accent)" style={S.tileIcon} />
          <div style={S.tileText}>
            <div style={S.tileLabel}>Backups kept</div>
            <div style={S.tileValue}>{settings ? `${regular.length} of ${settings.keep}` : '—'}</div>
            <div style={S.tileSub}>{`${formatBytes(backups.reduce((sum, b) => sum + b.sizeBytes, 0))} in total`}</div>
          </div>
        </div>
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, marginBottom: 12 }}>
        <button
          type="button"
          style={{ ...hybridControlStyles.primaryButton, ...(running ? hybridControlStyles.disabled : null) }}
          onClick={() => { void backUpNow(); }}
          disabled={Boolean(running)}
        >
          <I.Download size={16} />Back up now
        </button>
        <button type="button" style={hybridControlStyles.secondaryButton} onClick={() => setPicker('restore-from')} disabled={Boolean(running)}>
          <I.FolderUp size={16} />Restore from another folder…
        </button>
      </div>
      {notice && <div role="alert" style={{ ...S.muted, color: 'var(--danger)', marginBottom: 12 }}>{notice}</div>}

      {draft && (
        <div style={S.card} data-backup-card="schedule">
          <div style={S.cardTitle}>Schedule</div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 20, alignItems: 'flex-end' }}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              <div style={S.fieldLabel} id="backup-schedule-label">How often</div>
              <div role="group" aria-labelledby="backup-schedule-label" style={S.segments}>
                {SCHEDULE_OPTIONS.map((option) => {
                  const active = draft.schedule === option.value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      aria-pressed={active}
                      style={{ ...S.segment, ...(active ? S.segmentActive : null) }}
                      onClick={() => { setDraft({ ...draft, schedule: option.value }); setScheduleSaved(null); }}
                    >
                      {option.icon}{option.label}
                    </button>
                  );
                })}
              </div>
            </div>
            {draft.schedule === 'weekly' && (
              <label style={S.fieldColumn}>Day
                <select
                  value={draft.day}
                  onChange={(e) => { setDraft({ ...draft, day: Number(e.target.value) }); setScheduleSaved(null); }}
                  style={S.input}
                >
                  {SCHEDULE_DAYS.map((day, index) => <option key={day} value={index}>{day}</option>)}
                </select>
              </label>
            )}
            {draft.schedule !== 'off' && (
              <label style={S.fieldColumn}>Time
                <input
                  type="time"
                  value={draft.time}
                  onChange={(e) => { setDraft({ ...draft, time: e.target.value }); setScheduleSaved(null); }}
                  style={S.input}
                />
              </label>
            )}
            <label style={S.fieldColumn}>Keep
              <input
                type="number"
                min={1}
                max={30}
                value={draft.keep}
                onChange={(e) => { setDraft({ ...draft, keep: Number(e.target.value) }); setScheduleSaved(null); }}
                style={{ ...S.input, width: 84 }}
              />
            </label>
            <button
              type="button"
              style={hybridControlStyles.secondaryButton}
              onClick={() => { if (settings) void save({ ...settings, schedule: draft.schedule, day: draft.day, time: draft.time, keep: draft.keep }, setScheduleSaved); }}
            >
              Save
            </button>
            {scheduleSaved && <span role="status" style={S.muted}>{scheduleSaved}</span>}
          </div>
          <div style={S.muted}>
            The oldest backup is deleted when a new one would go over the limit. Backups made before an upgrade are kept
            separately (the last 2).
          </div>
        </div>
      )}

      {draft && (
        <div style={S.card} data-backup-card="folder">
          <div style={S.cardTitle}>Backup folder</div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
            <input
              type="text"
              aria-label="Backup folder"
              value={draft.path || ''}
              placeholder={overview?.folder}
              onChange={(e) => { setDraft({ ...draft, path: e.target.value }); setFolderSaved(null); }}
              style={{ ...S.input, ...S.mono, flex: '1 1 360px', minWidth: 0 }}
            />
            <button type="button" style={hybridControlStyles.secondaryButton} onClick={() => setPicker('folder')}>Browse…</button>
            <button
              type="button"
              style={hybridControlStyles.secondaryButton}
              onClick={() => { if (settings) void save({ ...settings, path: draft.path }, setFolderSaved); }}
            >
              Save
            </button>
            {folderSaved && <span role="status" style={S.muted}>{folderSaved}</span>}
          </div>
          <div style={{ display: 'flex', gap: 10, alignItems: 'flex-start', ...S.muted }}>
            <I.Lock size={16} color="var(--warning, #e3b05f)" style={{ flexShrink: 0, marginTop: 2 }} />
            <span>
              A backup holds your database and the key that protects Subsonic and scrobbling passwords, so keep this folder as
              private as the data folder. A folder on another disk also survives a disk failure.
            </span>
          </div>
        </div>
      )}

      {settings && (
        <div style={S.card} data-backup-card="include">
          <div style={S.cardTitle}>What to include</div>
          <div style={S.muted}>Every backup has the database, the password key and artwork you uploaded or picked. Music files are never copied.</div>
          <label style={S.toggleRow}>
            <span style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
              <span style={S.toggleTitle}>All original artwork</span>
              <span style={S.muted}>Also copy artwork fetched from providers. It can be fetched again, so this is off by default.</span>
            </span>
            <input
              type="checkbox"
              checked={settings.includeAllArtwork}
              onChange={(e) => { void save({ ...settings, includeAllArtwork: e.target.checked }, () => {}); }}
              style={S.checkbox}
            />
          </label>
          <label style={S.toggleRow}>
            <span style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
              <span style={S.toggleTitle}>Rendered mixes</span>
              <span style={S.muted}>Copy BoogieMix audio files. These can be large.</span>
            </span>
            <input
              type="checkbox"
              checked={settings.includeMixes}
              onChange={(e) => { void save({ ...settings, includeMixes: e.target.checked }, () => {}); }}
              style={S.checkbox}
            />
          </label>
        </div>
      )}

      <div style={S.card}>
        <div style={S.cardTitle}>Backups</div>
        {running?.kind === 'backup' && (
          <div style={S.runningRow}>
            <I.Spinner size={18} color="var(--accent)" />
            <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 6 }}>
              <div role="status" style={{ fontSize: 14, fontWeight: 650 }}>Backing up… {running.step.toLowerCase()}</div>
              <div style={S.track}><div style={S.trackBar} /></div>
            </div>
          </div>
        )}
        {backups.map((b) => {
          const date = formatBackupDate(b.createdAt);
          return (
            <div key={b.name} data-backup-name={b.name} style={S.row}>
              <div style={{ flex: '1 1 260px', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 4 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                  <span style={{ fontSize: 14, fontWeight: 700 }}>{date}</span>
                  <span style={S.chip}>{BACKUP_REASON_LABELS[b.reason]}</span>
                </div>
                <div style={S.tileSub}>v{b.appVersion} · {formatBytes(b.sizeBytes)}</div>
              </div>
              <div style={S.counts}>
                <span style={S.count}><I.Tracks size={14} color="var(--accent)" style={S.countIcon} />{b.counts.tracks.toLocaleString('en-GB')} tracks</span>
                <span style={S.count}><I.Playlists size={14} color="var(--accent)" style={S.countIcon} />{b.counts.playlists.toLocaleString('en-GB')} playlists</span>
                <span style={S.count}><I.Users size={14} color="var(--accent)" style={S.countIcon} />{b.counts.users.toLocaleString('en-GB')} users</span>
                <span style={S.count}><I.Artwork size={14} color="var(--accent)" style={S.countIcon} />{b.artworkCount} artwork</span>
                {b.keyIncluded && <span style={S.count}><I.Key size={14} color="var(--accent)" style={S.countIcon} />Passwords included</span>}
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button
                  type="button"
                  style={{ ...hybridControlStyles.tonalButton, minHeight: 34, padding: '6px 12px', fontSize: 13 }}
                  onClick={() => { setRestoreError(null); setRestore({ summary: b, source: { name: b.name } }); }}
                  disabled={Boolean(running)}
                >
                  <I.Restore size={14} />Restore
                </button>
                <button
                  type="button"
                  aria-label={`Delete backup from ${date}`}
                  style={S.deleteButton}
                  onClick={() => setDeleting(b)}
                  disabled={Boolean(running)}
                >
                  <I.Trash size={15} />
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {picker === 'folder' && draft && (
        <FolderPickerModal
          initialPath={draft.path || overview?.folder || undefined}
          onSelect={(path) => { setDraft({ ...draft, path }); setFolderSaved(null); setPicker(null); }}
          onClose={() => setPicker(null)}
        />
      )}
      {picker === 'restore-from' && (
        <FolderPickerModal
          initialPath={overview?.folder || undefined}
          onSelect={(path) => { void openRestoreFrom(path); }}
          onClose={() => setPicker(null)}
        />
      )}
      {restore && (
        <RestoreBackupDialog
          backup={restore.summary}
          busy={restoreBusy}
          error={restoreError}
          onConfirm={(remaps) => { void confirmRestore(remaps); }}
          onCancel={() => { if (!restoreBusy) setRestore(null); }}
        />
      )}
      {deleting && (
        <ConfirmModal
          title="Delete backup"
          message={`The backup from ${formatBackupDate(deleting.createdAt)} is deleted from the backup folder.`}
          confirmLabel="Delete"
          tone="danger"
          onConfirm={() => { void confirmDelete(); }}
          onCancel={() => setDeleting(null)}
        />
      )}
    </>
  );
}

const S: Record<string, React.CSSProperties> = {
  card: {
    padding: '16px 20px', borderRadius: 8, marginBottom: 12, backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)', display: 'flex', flexDirection: 'column', gap: 14,
  },
  cardTitle: { fontSize: 15, fontWeight: 600, color: 'var(--text)' },
  failure: {
    display: 'flex', gap: 12, alignItems: 'flex-start', padding: '14px 16px', borderRadius: 12, marginBottom: 12,
    border: '1px solid var(--danger, #f07b76)', background: 'color-mix(in srgb, var(--danger, #f07b76) 12%, var(--surface))',
  },
  muted: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.55 },
  tiles: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12, marginBottom: 12 },
  tile: {
    display: 'flex', gap: 12, alignItems: 'center', padding: 16, borderRadius: 12,
    background: 'var(--surface)', border: '1px solid var(--border)',
  },
  tileIcon: { opacity: 0.85, flexShrink: 0 },
  tileText: { display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 },
  tileLabel: { fontSize: 12, color: 'var(--text-muted)', fontWeight: 600 },
  tileValue: { fontSize: 16, fontWeight: 700, color: 'var(--text)' },
  tileSub: { fontSize: 12, color: 'var(--text-muted)' },
  fieldLabel: { fontSize: 13, color: 'var(--text-muted)' },
  fieldColumn: { display: 'flex', flexDirection: 'column', gap: 6, fontSize: 13, color: 'var(--text-muted)' },
  segments: { display: 'inline-flex', padding: 3, gap: 3, borderRadius: 11, background: 'var(--surface-subtle, var(--bg))' },
  segment: {
    minHeight: 34, display: 'inline-flex', alignItems: 'center', gap: 6, padding: '6px 12px', border: 'none', borderRadius: 8,
    background: 'transparent', color: 'var(--text-muted)', fontFamily: 'inherit', fontSize: 13, fontWeight: 650, cursor: 'pointer',
  },
  segmentActive: { background: 'var(--accent-soft, var(--surface))', color: 'var(--accent)', fontWeight: 700 },
  input: {
    minHeight: 38, padding: '6px 10px', borderRadius: 8, border: '1px solid var(--border-strong, var(--border))',
    background: 'var(--bg)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 14,
  },
  mono: { fontFamily: "'IBM Plex Mono', monospace", fontSize: 13 },
  toggleRow: { display: 'flex', justifyContent: 'space-between', gap: 16, alignItems: 'center', cursor: 'pointer' },
  toggleTitle: { fontSize: 14, fontWeight: 650, color: 'var(--text)' },
  checkbox: { width: 20, height: 20, accentColor: 'var(--accent)', flexShrink: 0 },
  runningRow: { display: 'flex', alignItems: 'center', gap: 12, padding: 12, borderRadius: 10, background: 'var(--surface-raised, var(--bg))' },
  track: { height: 6, borderRadius: 3, background: 'var(--border)', overflow: 'hidden' },
  trackBar: { width: '60%', height: '100%', background: 'var(--accent)' },
  row: { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 14, padding: 12, borderTop: '1px solid var(--divider-subtle, var(--border))' },
  chip: {
    display: 'inline-flex', alignItems: 'center', padding: '2px 8px', borderRadius: 999,
    background: 'var(--surface-subtle, var(--bg))', color: 'var(--text-muted)', fontSize: 12, fontWeight: 600,
  },
  counts: { display: 'flex', flexWrap: 'wrap', gap: 12, fontSize: 13, color: 'var(--text-muted)' },
  count: { display: 'inline-flex', alignItems: 'center', gap: 5 },
  countIcon: { opacity: 0.85 },
  deleteButton: {
    minHeight: 34, width: 36, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', border: '1px solid transparent',
    borderRadius: 9, background: 'var(--surface-subtle, var(--bg))', color: 'var(--danger, #f07b76)', cursor: 'pointer',
  },
};
