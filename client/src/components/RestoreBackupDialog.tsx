/**
 * "Restore this backup?" confirmation: the backup's summary, the checks that passed, new locations for music
 * folders this server doesn't have, and what the restore replaces.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { BackupSummary, PathRemap } from '../types';
import FolderPickerModal from './FolderPickerModal';
import { BackupIcons as I, formatBackupDate, preRestoreFolderName } from './backupUi';
import { hybridControlStyles } from '../hybridPreview';

interface Props {
  backup: BackupSummary;
  busy?: boolean;
  error?: string | null;
  onConfirm: (remaps: PathRemap[]) => void;
  onCancel: () => void;
}

export default function RestoreBackupDialog({ backup, busy = false, error = null, onConfirm, onCancel }: Props) {
  const [targets, setTargets] = useState<Record<string, string>>({});
  const [picking, setPicking] = useState<string | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const savedFolder = useMemo(() => preRestoreFolderName(), []);
  const foundRoots = backup.libraryRoots.filter((root) => !backup.missingRoots.includes(root));
  const remaps: PathRemap[] = backup.missingRoots
    .map((from) => ({ from, to: (targets[from] ?? '').trim() }))
    .filter((remap) => remap.to);
  const ready = remaps.length === backup.missingRoots.length && !busy;

  useEffect(() => { cancelRef.current?.focus(); }, []);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy && !picking) onCancel();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [busy, onCancel, picking]);

  const checks = [
    'The database is intact',
    'The checksum matches',
    'Made by this version or an older one',
    ...(backup.keyIncluded ? ['The password key is included'] : []),
  ];
  const tiles: Array<{ icon: React.ReactNode; label: string; value: string }> = [
    { icon: <I.Calendar size={20} />, label: 'Created', value: formatBackupDate(backup.createdAt, { weekday: false }) },
    { icon: <I.Tag size={20} />, label: 'Version', value: backup.appVersion },
    { icon: <I.Tracks size={20} />, label: 'Tracks', value: backup.counts.tracks.toLocaleString('en-GB') },
    { icon: <I.Playlists size={20} />, label: 'Playlists', value: backup.counts.playlists.toLocaleString('en-GB') },
    { icon: <I.Users size={20} />, label: 'Users', value: backup.counts.users.toLocaleString('en-GB') },
    { icon: <I.Artwork size={20} />, label: 'Artwork', value: `${backup.artworkCount} ${backup.artworkCount === 1 ? 'file' : 'files'}` },
  ];

  return (
    <div role="presentation" style={S.overlay} onClick={(event) => { if (event.target === event.currentTarget && !busy) onCancel(); }}>
      <div role="alertdialog" aria-modal="true" aria-labelledby="restore-title" style={S.card}>
        <div style={{ padding: '20px 24px 6px' }}>
          <div style={S.eyebrow}>Restore</div>
          <div id="restore-title" style={S.title}>Restore this backup?</div>
        </div>
        <div style={S.body}>
          <div style={S.tiles}>
            {tiles.map((tile) => (
              <div key={tile.label} style={S.tile}>
                <span style={S.tileIcon}>{tile.icon}</span>
                <div><div style={S.tileLabel}>{tile.label}</div><div style={S.tileValue}>{tile.value}</div></div>
              </div>
            ))}
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            <div style={S.sectionLabel}>Checks</div>
            {checks.map((check) => (
              <div key={check} style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 14 }}>
                <I.CheckCircle size={18} color="var(--success, #65c590)" />
                <span>{check}</span>
              </div>
            ))}
          </div>

          {backup.missingRoots.length > 0 && (
            <div style={S.remapBox}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <I.FolderAlert size={18} color="var(--warning, #e3b05f)" />
                <div style={{ fontSize: 15, fontWeight: 700 }}>Music folders not found on this server</div>
              </div>
              <div style={S.muted}>Choose where each library folder is now. Tracks are matched on the next scan. A wrong folder removes nothing.</div>
              {backup.missingRoots.map((root, index) => {
                const id = `remap-${index}`;
                return (
                  <div key={root} style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                    <div style={S.fieldLabel}>Was</div>
                    <div style={S.mono}>{root}</div>
                    <label htmlFor={id} style={{ ...S.fieldLabel, marginTop: 6 }}>Now</label>
                    <div style={{ display: 'flex', gap: 8 }}>
                      <input
                        id={id}
                        type="text"
                        value={targets[root] ?? ''}
                        onChange={(event) => setTargets((prev) => ({ ...prev, [root]: event.target.value }))}
                        style={S.input}
                        disabled={busy}
                      />
                      <button type="button" style={hybridControlStyles.secondaryButton} onClick={() => setPicking(root)} disabled={busy}>Browse…</button>
                    </div>
                  </div>
                );
              })}
              {foundRoots.map((root) => (
                <div key={root} style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, color: 'var(--text-muted)' }}>
                  <I.Check size={16} color="var(--success, #65c590)" />
                  <span style={S.mono}>{root}</span><span>found</span>
                </div>
              ))}
            </div>
          )}

          <div style={S.warning}>
            <I.Warning size={18} color="var(--danger, #f07b76)" style={{ flexShrink: 0, marginTop: 2 }} />
            <span>
              Everything changed since this backup is replaced: plays, ratings, playlists, users and settings. Your current
              data is saved first to <span style={{ ...S.mono, fontSize: 13 }}>{savedFolder}</span> in the data folder. Everyone,
              including you, has to sign in again.
            </span>
          </div>
          {error && <div role="alert" style={{ color: 'var(--danger, #f07b76)', fontSize: 14 }}>{error}</div>}
        </div>
        <div style={S.footer}>
          <button ref={cancelRef} type="button" style={S.cancel} onClick={onCancel} disabled={busy}>Cancel</button>
          <button
            type="button"
            style={{ ...S.confirm, ...(ready ? null : hybridControlStyles.disabled) }}
            onClick={() => onConfirm(remaps)}
            disabled={!ready}
          >
            Restore
          </button>
        </div>
      </div>
      {picking && (
        <FolderPickerModal
          initialPath={targets[picking] || undefined}
          onSelect={(path) => { setTargets((prev) => ({ ...prev, [picking]: path })); setPicking(null); }}
          onClose={() => setPicking(null)}
        />
      )}
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  overlay: {
    position: 'fixed', inset: 0, zIndex: 2000, background: 'rgba(0,0,0,0.66)',
    display: 'flex', alignItems: 'flex-start', justifyContent: 'center', padding: 24, overflowY: 'auto',
  },
  card: {
    width: '100%', maxWidth: 680, background: 'var(--surface)', border: '1px solid var(--border)',
    borderRadius: 14, overflow: 'hidden', display: 'flex', flexDirection: 'column', color: 'var(--text)',
  },
  eyebrow: { fontSize: 11, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: 'var(--danger, #f07b76)', marginBottom: 4 },
  title: { fontSize: 19, fontWeight: 750 },
  body: { padding: '14px 24px 18px', display: 'flex', flexDirection: 'column', gap: 18 },
  tiles: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))', gap: 10 },
  tile: { display: 'flex', gap: 10, alignItems: 'center', padding: 12, borderRadius: 10, background: 'var(--surface-raised, var(--bg))' },
  tileIcon: { color: 'var(--accent)', opacity: 0.85, display: 'inline-flex', flexShrink: 0 },
  tileLabel: { fontSize: 12, color: 'var(--text-muted)' },
  tileValue: { fontSize: 14, fontWeight: 700 },
  sectionLabel: { fontSize: 13, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.8 },
  remapBox: {
    display: 'flex', flexDirection: 'column', gap: 10, padding: 14, borderRadius: 12,
    border: '1px solid var(--border-strong, var(--border))', background: 'var(--surface-raised, var(--bg))',
  },
  muted: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.55 },
  fieldLabel: { fontSize: 12, color: 'var(--text-muted)' },
  mono: { fontFamily: "'IBM Plex Mono', monospace", fontSize: 13, wordBreak: 'break-all' },
  input: {
    flex: 1, minWidth: 0, minHeight: 38, padding: '7px 10px', borderRadius: 8, border: '1px solid var(--border-strong, var(--border))',
    background: 'var(--bg)', color: 'var(--text)', fontFamily: "'IBM Plex Mono', monospace", fontSize: 13,
  },
  warning: {
    display: 'flex', gap: 10, alignItems: 'flex-start', padding: '12px 14px', borderRadius: 10,
    background: 'color-mix(in srgb, var(--danger, #f07b76) 12%, transparent)', fontSize: 14, lineHeight: 1.55,
  },
  footer: { padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'flex-end', gap: 10 },
  cancel: {
    minHeight: 40, padding: '8px 18px', border: '1px solid var(--border)', borderRadius: 8, background: 'var(--bg)',
    color: 'var(--text)', fontSize: 15, fontFamily: 'inherit', cursor: 'pointer',
  },
  confirm: {
    minHeight: 40, padding: '8px 18px', border: 'none', borderRadius: 8, background: 'var(--danger, #f07b76)',
    color: 'var(--on-accent, #fff)', fontSize: 15, fontWeight: 700, fontFamily: 'inherit', cursor: 'pointer',
  },
};
