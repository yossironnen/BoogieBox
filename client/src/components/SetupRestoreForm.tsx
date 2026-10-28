/**
 * First-run "Restore from a backup": choose the backup, an empty database folder, and where each music folder
 * is on this machine.
 */

import React, { useState } from 'react';
import { api } from '../api';
import type { BackupSummary, PathRemap } from '../types';
import { hybridControlStyles, hybridEntryStyles } from '../hybridPreview';
import { BackupIcons as I, formatBackupDate } from './backupUi';

interface Props {
  defaultDbFolder: string;
  pickFolder: (initial: string) => Promise<string | null>;
  onComplete: () => void;
}

export default function SetupRestoreForm({ defaultDbFolder, pickFolder, onComplete }: Props) {
  const [backupPath, setBackupPath] = useState('');
  const [summary, setSummary] = useState<BackupSummary | null>(null);
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [dbFolder, setDbFolder] = useState(defaultDbFolder);
  const [targets, setTargets] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const inspect = async (path: string) => {
    const trimmed = path.trim();
    if (!trimmed) return;
    setInspectError(null);
    try {
      const found = await api.backups.setupInspect(trimmed);
      setSummary(found);
      setTargets(Object.fromEntries(found.libraryRoots.map((root) => [root, found.missingRoots.includes(root) ? '' : root])));
    } catch (e) {
      setSummary(null);
      setInspectError(e instanceof Error ? e.message : String(e));
    }
  };

  const browse = async (initial: string, apply: (path: string) => void) => {
    const picked = await pickFolder(initial);
    if (picked) apply(picked);
  };

  const remaps: PathRemap[] = summary
    ? summary.libraryRoots.map((from) => ({ from, to: (targets[from] ?? '').trim() }))
    : [];
  const unmapped = remaps.some((remap) => !remap.to);
  const step = !summary ? 1 : !dbFolder.trim() ? 2 : 3;
  const ready = Boolean(summary) && Boolean(dbFolder.trim()) && !unmapped && !busy;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!ready) return;
    setBusy(true);
    setError(null);
    try {
      await api.backups.setupRestore(backupPath.trim(), dbFolder.trim(), remaps.filter((remap) => remap.to !== remap.from));
      onComplete();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const steps = ['1. Choose backup', '2. Choose storage', '3. Map music folders', '4. Sign in'];

  return (
    <>
      <ol aria-label="Setup progress" style={{ ...hybridEntryStyles.progress, listStyle: 'none', padding: 0 }}>
        {steps.map((label, index) => (
          <li
            key={label}
            aria-current={index + 1 === step ? 'step' : undefined}
            style={{ ...hybridEntryStyles.progressItem, ...(index + 1 === step ? hybridEntryStyles.progressItemActive : null) }}
          >
            {label}
          </li>
        ))}
      </ol>

      <form onSubmit={submit} style={{ ...hybridEntryStyles.content, display: 'flex', flexDirection: 'column', gap: 20 }}>
        <div style={S.group}>
          <label htmlFor="setup-backup-folder" style={S.label}>Backup folder</label>
          <div style={S.row}>
            <input
              id="setup-backup-folder"
              type="text"
              value={backupPath}
              onChange={(event) => { setBackupPath(event.target.value); setSummary(null); }}
              onBlur={() => { void inspect(backupPath); }}
              disabled={busy}
              style={S.field}
            />
            <button
              type="button"
              style={{ ...hybridControlStyles.secondaryButton, minWidth: 92 }}
              disabled={busy}
              onClick={() => { void browse(backupPath, (path) => { setBackupPath(path); void inspect(path); }); }}
            >
              Browse
            </button>
          </div>
          {summary && (
            <div style={S.facts}>
              <span style={S.fact}><I.Check size={14} color="var(--success, #65c590)" />Backup from {formatBackupDate(summary.createdAt, { weekday: false })} (v{summary.appVersion})</span>
              <span style={S.fact}><I.Tracks size={14} color="var(--accent)" style={{ opacity: 0.85 }} />{summary.counts.tracks.toLocaleString('en-GB')} tracks</span>
              <span style={S.fact}><I.Users size={14} color="var(--accent)" style={{ opacity: 0.85 }} />{summary.counts.users.toLocaleString('en-GB')} users</span>
            </div>
          )}
          {inspectError && <div role="alert" style={hybridEntryStyles.error}>{inspectError}</div>}
        </div>

        <div style={S.group}>
          <label htmlFor="setup-restore-db-folder" style={S.label}>Database folder</label>
          <div style={S.row}>
            <input
              id="setup-restore-db-folder"
              type="text"
              value={dbFolder}
              onChange={(event) => setDbFolder(event.target.value)}
              disabled={busy}
              style={S.field}
            />
            <button
              type="button"
              style={{ ...hybridControlStyles.secondaryButton, minWidth: 92 }}
              disabled={busy}
              onClick={() => { void browse(dbFolder, setDbFolder); }}
            >
              Browse
            </button>
          </div>
          <div style={hybridEntryStyles.technicalNote}>An empty folder. BoogieBox keeps the restored database and settings here.</div>
        </div>

        {summary && summary.libraryRoots.length > 0 && (
          <div style={S.mapBox}>
            <div style={{ fontSize: 14, fontWeight: 750 }}>Where is your music on this machine?</div>
            <div style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.55 }}>
              Choose the folder for each library. Tracks are matched on the first scan, keeping their plays, ratings and playlists.
            </div>
            {summary.libraryRoots.map((root) => {
              const value = targets[root] ?? '';
              return (
                <div key={root} style={S.group}>
                  <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                    Was <span style={{ ...S.mono, color: 'var(--text)' }}>{root}</span>
                  </div>
                  <div style={S.row}>
                    <input
                      type="text"
                      aria-label={`New folder for ${root}`}
                      value={value}
                      placeholder="Choose a folder"
                      onChange={(event) => setTargets((prev) => ({ ...prev, [root]: event.target.value }))}
                      disabled={busy}
                      style={{ ...S.field, ...(value.trim() ? null : { borderColor: 'var(--danger)' }) }}
                    />
                    <button
                      type="button"
                      style={{ ...hybridControlStyles.secondaryButton, minWidth: 92 }}
                      disabled={busy}
                      onClick={() => { void browse(value, (path) => setTargets((prev) => ({ ...prev, [root]: path }))); }}
                    >
                      Browse
                    </button>
                  </div>
                  {!value.trim() && <div role="alert" style={{ fontSize: 13, color: 'var(--danger)' }}>Choose a folder that exists on this machine.</div>}
                </div>
              );
            })}
          </div>
        )}

        {error && <div role="alert" style={hybridEntryStyles.error}>{error}</div>}

        <button
          type="submit"
          disabled={!ready}
          style={{ ...hybridControlStyles.primaryButton, width: '100%', ...(ready ? null : hybridControlStyles.disabled) }}
        >
          Restore BoogieBox
        </button>
      </form>
    </>
  );
}

const S: Record<string, React.CSSProperties> = {
  group: { display: 'flex', flexDirection: 'column', gap: 8 },
  label: { display: 'block', color: 'var(--text)', fontSize: 14, fontWeight: 750 },
  row: { display: 'flex', gap: 8 },
  field: { ...hybridControlStyles.field, flex: 1, minWidth: 0, boxSizing: 'border-box', fontFamily: "'IBM Plex Mono', monospace", fontSize: 13 },
  facts: { display: 'flex', flexWrap: 'wrap', gap: 12, fontSize: 13, color: 'var(--text-muted)' },
  fact: { display: 'inline-flex', alignItems: 'center', gap: 5 },
  mapBox: {
    display: 'flex', flexDirection: 'column', gap: 12, padding: 16, borderRadius: 12,
    background: 'var(--surface-raised, var(--surface))', border: '1px solid var(--border)',
  },
  mono: { fontFamily: "'IBM Plex Mono', monospace" },
};
