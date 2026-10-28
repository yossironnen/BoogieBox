/**
 * Mobile admin › Backup: status, "Back up now" and the list (delete only). The schedule and restoring stay on
 * the desktop site.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../../api';
import type { BackupsOverview, BackupSummary } from '../../types';
import {
  BACKUP_REASON_LABELS,
  BackupIcons as I,
  formatBackupDate,
  formatBytes,
  formatNextRun,
  formatRecentBackupDate,
} from '../../components/backupUi';
import MobileConfirmationSheet from './MobileConfirmationSheet';

const POLL_MS = 1500;

export default function MobileBackupSection() {
  const [overview, setOverview] = useState<BackupsOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<BackupSummary | null>(null);

  const load = useCallback(async () => {
    try {
      setOverview(await api.backups.overview());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);
  const running = overview?.running ?? null;
  useEffect(() => {
    if (!running) return undefined;
    const id = window.setInterval(() => { void load(); }, POLL_MS);
    return () => window.clearInterval(id);
  }, [load, running]);

  const regular = (overview?.backups ?? []).filter((b) => b.reason === 'scheduled' || b.reason === 'manual');
  const newest = regular[0] ?? null;

  return (
    <div style={S.section} aria-label="Backup">
      <div style={S.title}>Backup</div>
      <div style={S.tiles}>
        <div style={S.tile}>
          <I.LastBackup size={22} color="var(--accent)" style={{ opacity: 0.85 }} />
          <div style={S.tileLabel}>Last backup</div>
          <div style={S.tileValue}>{running?.kind === 'backup' ? 'Backing up…' : newest ? formatRecentBackupDate(newest.createdAt) : '—'}</div>
        </div>
        <div style={S.tile}>
          <I.Calendar size={22} color="var(--accent)" style={{ opacity: 0.85 }} />
          <div style={S.tileLabel}>Next backup</div>
          <div style={S.tileValue}>{!overview?.nextRun || overview.settings.schedule === 'off' ? 'Off' : formatNextRun(overview.nextRun)}</div>
        </div>
      </div>
      <button
        type="button"
        style={{ ...S.primary, ...(running ? { opacity: 0.55 } : null) }}
        disabled={Boolean(running)}
        onClick={() => {
          void api.backups.create().catch((e: unknown) => setError(e instanceof Error ? e.message : String(e))).finally(() => { void load(); });
        }}
      >
        <I.Download size={16} />Back up now
      </button>
      {error && <div role="alert" style={{ color: 'var(--danger)', fontSize: 13 }}>{error}</div>}
      {overview && overview.backups.length > 0 && (
        <div style={S.list}>
          <div style={S.listTitle}>Backups</div>
          {overview.backups.map((b) => {
            const date = formatBackupDate(b.createdAt, { year: false });
            return (
              <div key={b.name} style={S.row}>
                <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 3 }}>
                  <div style={{ fontSize: 14, fontWeight: 700 }}>{date}</div>
                  <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                    {BACKUP_REASON_LABELS[b.reason]} · v{b.appVersion} · {formatBytes(b.sizeBytes)}
                  </div>
                </div>
                <button type="button" aria-label={`Delete backup from ${date}`} style={S.iconButton} onClick={() => setDeleting(b)}>
                  <I.Trash size={16} />
                </button>
              </div>
            );
          })}
        </div>
      )}
      <div style={S.note}>
        <I.Desktop size={16} color="var(--accent)" style={{ flexShrink: 0, marginTop: 2 }} />
        <span>Change the schedule or restore a backup from BoogieBox in a desktop browser.</span>
      </div>
      <MobileConfirmationSheet
        open={deleting !== null}
        title="Delete backup"
        description={deleting ? `The backup from ${formatBackupDate(deleting.createdAt)} is deleted from the backup folder.` : ''}
        confirmLabel="Delete"
        tone="danger"
        onClose={() => setDeleting(null)}
        onConfirm={async () => {
          if (!deleting) return;
          await api.backups.remove(deleting.name);
          setDeleting(null);
          await load();
        }}
      />
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  section: { display: 'grid', gap: 10, paddingTop: 12, borderTop: '1px solid var(--divider-subtle)' },
  title: { color: 'var(--text)', fontSize: 14, fontWeight: 800 },
  tiles: { display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 10 },
  tile: {
    display: 'flex', flexDirection: 'column', gap: 6, padding: 14, borderRadius: 14,
    background: 'var(--surface)', border: '1px solid var(--border)',
  },
  tileLabel: { fontSize: 12, color: 'var(--text-muted)', fontWeight: 600 },
  tileValue: { fontSize: 15, fontWeight: 700, color: 'var(--text)' },
  primary: {
    minHeight: 48, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 8, border: 'none',
    borderRadius: 12, background: 'var(--accent)', color: 'var(--on-accent, #fff)', fontFamily: 'inherit', fontSize: 15, fontWeight: 700,
  },
  list: { display: 'flex', flexDirection: 'column', borderRadius: 14, background: 'var(--surface)', border: '1px solid var(--border)', overflow: 'hidden' },
  listTitle: { padding: '14px 14px 8px', fontSize: 15, fontWeight: 700 },
  row: { display: 'flex', alignItems: 'center', gap: 10, padding: '12px 14px', borderTop: '1px solid var(--divider-subtle)' },
  iconButton: {
    width: 44, height: 44, flexShrink: 0, borderRadius: 12, border: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)', color: 'var(--danger)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  },
  note: {
    display: 'flex', gap: 10, alignItems: 'flex-start', padding: '12px 14px', borderRadius: 12,
    background: 'var(--surface-raised, var(--surface))', fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.55,
  },
};
