/**
 * Playlist import (M3U/M3U8): the preview state, the shared preview body (desktop dialog and mobile sheet),
 * the desktop dialog, and the "Playlist imported" toast.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import type { PathRemap, Playlist, PlaylistImportPreview, PlaylistImportRemap } from '../types';
import {
  IMPORT_PROBLEM_COPY,
  MAX_IMPORT_BYTES,
  NAME_TAKEN_MESSAGE,
  candidateDetail,
  downloadTextFile,
  importSummary,
  importTrackIds,
  initialChoices,
  isImportProblem,
  notFoundText,
  playlistNameFromFile,
  readFileBase64,
  trackCount,
  type ImportChoices,
  type ImportProblem,
} from '../playlistImport';
import { hybridControlStyles } from '../hybridPreview';

// ─── Icons ────────────────────────────────────────────────────────────────────

type IconProps = { size?: number; color?: string; style?: React.CSSProperties };

function Svg({ size = 16, color = 'currentColor', style, children, strokeWidth = 2 }: IconProps & { children: React.ReactNode; strokeWidth?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={style}>
      {children}
    </svg>
  );
}

export const ImportIcons = {
  ImportFile: (p: IconProps) => <Svg {...p}><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /><path d="M12 12v6M9 15l3 3 3-3" /></Svg>,
  File: (p: IconProps) => <Svg {...p}><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></Svg>,
  Found: (p: IconProps) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="m8 12 3 3 5-6" /></Svg>,
  ChooseOne: (p: IconProps) => <Svg {...p}><circle cx="6" cy="5" r="2" /><circle cx="18" cy="5" r="2" /><circle cx="12" cy="19" r="2" /><path d="M6 7v1a3 3 0 0 0 3 3h6a3 3 0 0 0 3-3V7M12 11v6" /></Svg>,
  NotFound: (p: IconProps) => <Svg {...p}><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /><path d="m8.5 8.5 5 5M13.5 8.5l-5 5" /></Svg>,
  WebLink: (p: IconProps) => <Svg {...p}><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></Svg>,
  Repeat: (p: IconProps) => <Svg {...p}><path d="m17 2 4 4-4 4" /><path d="M3 11V9a3 3 0 0 1 3-3h15" /><path d="m7 22-4-4 4-4" /><path d="M21 13v2a3 3 0 0 1-3 3H3" /></Svg>,
  FolderSwap: (p: IconProps) => <Svg {...p}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><path d="m10 13 2 2 2-2" /></Svg>,
  Arrow: (p: IconProps) => <Svg {...p}><path d="M5 12h14M13 6l6 6-6 6" /></Svg>,
  Check: (p: IconProps) => <Svg {...p} strokeWidth={2.4}><path d="m5 12 4 4 10-10" /></Svg>,
  Download: (p: IconProps) => <Svg {...p}><path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M5 21h14" /></Svg>,
  Playlist: (p: IconProps) => <Svg {...p}><path d="M3 6h13M3 12h13M3 18h9" /><path d="M19 15v6M16 18h6" /></Svg>,
  Warning: (p: IconProps) => <Svg {...p}><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /><path d="M12 9v4" /><path d="M12 17h.01" /></Svg>,
  Close: (p: IconProps) => <Svg {...p}><path d="M18 6 6 18M6 6l12 12" /></Svg>,
};
const I = ImportIcons;

// ─── State ────────────────────────────────────────────────────────────────────

export interface ImportedPlaylist {
  playlist: Playlist;
  added: number;
  leftOut: number;
}

export interface PlaylistImportState {
  fileName: string;
  lineCount: number;
  phase: 'loading' | 'ready' | 'problem';
  problem: ImportProblem | null;
  preview: PlaylistImportPreview | null;
  remaps: PathRemap[];
  choices: ImportChoices;
  name: string;
  nameError: string;
  error: string;
  busy: boolean;
  includedCount: number;
  canCreate: boolean;
  setName: (name: string) => void;
  choose: (line: number, trackId: string | null) => void;
  toggleRemap: (remap: PlaylistImportRemap) => void;
  downloadNotFound: () => void;
  create: () => Promise<ImportedPlaylist | null>;
}

/** Reads `file`, previews it, and keeps the folder swaps, choices and name until Create. */
export function usePlaylistImport(file: File): PlaylistImportState {
  const [phase, setPhase] = useState<PlaylistImportState['phase']>('loading');
  const [problem, setProblem] = useState<ImportProblem | null>(null);
  const [preview, setPreview] = useState<PlaylistImportPreview | null>(null);
  const [remaps, setRemaps] = useState<PathRemap[]>([]);
  const [choices, setChoices] = useState<ImportChoices>({});
  const [name, setNameState] = useState(() => playlistNameFromFile(file.name));
  const [nameError, setNameError] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(true);
  const contentRef = useRef<string | null>(null);

  const load = useCallback(async (nextRemaps: PathRemap[]) => {
    setBusy(true);
    setError('');
    try {
      if (contentRef.current === null) {
        if (file.size > MAX_IMPORT_BYTES) {
          setProblem('too_big');
          setPhase('problem');
          return;
        }
        contentRef.current = await readFileBase64(file);
      }
      const next = await api.playlists.importPreview(contentRef.current, nextRemaps);
      setRemaps(nextRemaps);
      setPreview(next);
      setChoices((previous) => initialChoices(next, previous));
      setPhase('ready');
    } catch (err) {
      // PlaylistImportError carries the server's reason; matched by shape so API mocks need not provide the class.
      const code = (err as { code?: string | null } | null)?.code;
      if (isImportProblem(code)) {
        setProblem(code);
        setPhase('problem');
      } else {
        setError(err instanceof Error && err.message ? err.message : 'Could not read this playlist.');
      }
    } finally {
      setBusy(false);
    }
  }, [file]);

  useEffect(() => { void load([]); }, [load]);

  const includedIds = preview ? importTrackIds(preview, choices) : [];
  const canCreate = phase === 'ready' && !busy && includedIds.length > 0 && name.trim().length > 0;

  const create = async (): Promise<ImportedPlaylist | null> => {
    if (!preview || !canCreate) return null;
    setBusy(true);
    setError('');
    setNameError('');
    try {
      const result = await api.playlists.importCommit(name.trim(), includedIds);
      return { playlist: result.playlist, added: result.added, leftOut: preview.entries.length - result.added };
    } catch (err) {
      const message = err instanceof Error ? err.message : '';
      if (message === NAME_TAKEN_MESSAGE) setNameError(message);
      else setError(message || 'Could not create playlist');
      return null;
    } finally {
      setBusy(false);
    }
  };

  return {
    fileName: file.name,
    lineCount: preview?.entries.length ?? 0,
    phase,
    problem,
    preview,
    remaps,
    choices,
    name,
    nameError,
    error,
    busy,
    includedCount: includedIds.length,
    canCreate,
    setName: (value) => { setNameState(value); setNameError(''); },
    choose: (line, trackId) => setChoices((prev) => ({ ...prev, [line]: trackId })),
    toggleRemap: (remap) => {
      const applied = remaps.some((r) => r.from === remap.from && r.to === remap.to);
      void load(applied
        ? remaps.filter((r) => !(r.from === remap.from && r.to === remap.to))
        : [...remaps, { from: remap.from, to: remap.to }]);
    },
    downloadNotFound: () => {
      if (preview) downloadTextFile(`${playlistNameFromFile(file.name) || 'playlist'} - not found.txt`, notFoundText(preview));
    },
    create,
  };
}

// ─── Shared body ──────────────────────────────────────────────────────────────

/** The preview body: file, name, tiles, folder swaps, choices and the not-found list. */
export function PlaylistImportBody({ state, compact = false }: { state: PlaylistImportState; compact?: boolean }) {
  const { preview } = state;
  const nameId = compact ? 'mobile-import-name' : 'import-name';

  if (state.phase === 'problem' && state.problem) {
    const copy = IMPORT_PROBLEM_COPY[state.problem];
    return (
      <div style={S.stack}>
        <FileLine state={state} />
        <div role="alert" style={S.problem}>
          <I.Warning size={18} color="var(--danger, #f07b76)" style={{ flexShrink: 0, marginTop: 2 }} />
          <div><div style={{ fontWeight: 700 }}>{copy.title}</div><div style={S.problemBody}>{copy.body}</div></div>
        </div>
      </div>
    );
  }

  const totals = preview?.totals;
  const tiles: Array<{ key: string; icon: React.ReactNode; label: string; value: number; attention?: boolean }> = totals ? [
    { key: 'matched', icon: <I.Found size={compact ? 16 : 20} />, label: 'Found', value: totals.matched },
    { key: 'ambiguous', icon: <I.ChooseOne size={compact ? 16 : 20} color="var(--warning, #e3b05f)" />, label: 'Choose one', value: totals.ambiguous, attention: totals.ambiguous > 0 },
    { key: 'unmatched', icon: <I.NotFound size={compact ? 16 : 20} />, label: 'Not found', value: totals.unmatched },
    { key: 'remote', icon: <I.WebLink size={compact ? 16 : 20} />, label: 'Web links', value: totals.remote },
    { key: 'duplicate', icon: <I.Repeat size={compact ? 16 : 20} />, label: 'Repeats', value: totals.duplicate },
  ] : [];
  const ambiguous = preview?.entries.filter((e) => e.status === 'ambiguous') ?? [];
  const missing = preview?.entries.filter((e) => e.status === 'unmatched') ?? [];
  const swaps = [
    ...(preview?.appliedRemaps ?? []).map((remap) => ({ remap, used: true })),
    ...(preview?.suggestedRemaps ?? []).map((remap) => ({ remap, used: false })),
  ];
  const nothingMatched = !!totals && totals.matched + totals.ambiguous + totals.duplicate === 0;

  return (
    <div style={S.stack}>
      <FileLine state={state} />

      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        <label htmlFor={nameId} style={S.fieldLabel}>Playlist name</label>
        <input
          id={nameId}
          type="text"
          value={state.name}
          onChange={(event) => state.setName(event.target.value)}
          aria-invalid={state.nameError ? true : undefined}
          aria-describedby={state.nameError ? `${nameId}-error` : undefined}
          style={{ ...S.input, ...(compact ? { minHeight: 44, fontSize: 16 } : null), ...(state.nameError ? { borderColor: 'var(--danger, #f07b76)' } : null) }}
        />
        {state.nameError && <div id={`${nameId}-error`} style={S.fieldError}>{state.nameError}</div>}
      </div>

      {state.phase === 'loading' && !preview && (
        <div role="status" style={S.muted}>Matching tracks…</div>
      )}

      {totals && (
        <div style={{ ...S.tiles, gridTemplateColumns: `repeat(${compact ? 3 : 5}, minmax(0, 1fr))` }} data-import-tiles="">
          {tiles.map((tile) => (
            <div key={tile.key} data-import-tile={tile.key} style={{ ...S.tile, ...(tile.attention ? S.tileAttention : null), ...(compact ? { padding: 10, gap: 4 } : null) }}>
              <span style={S.tileIcon}>{tile.icon}</span>
              <span style={{ ...S.tileValue, ...(compact ? { fontSize: 17 } : null) }}>{tile.value.toLocaleString('en-GB')}</span>
              <span style={S.tileLabel}>{tile.label}</span>
            </div>
          ))}
        </div>
      )}

      {swaps.length > 0 && (
        <section aria-labelledby={`${nameId}-swaps`} style={S.swapBox}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <I.FolderSwap size={18} color="var(--warning, #e3b05f)" />
            <div id={`${nameId}-swaps`} style={{ fontSize: 15, fontWeight: 700 }}>Some of this music is in a different folder here</div>
          </div>
          {!compact && (
            <div style={S.muted}>The file points to folders this server doesn&apos;t have. Use a library folder in their place to find more tracks.</div>
          )}
          {swaps.map(({ remap, used }) => (
            <div key={`${remap.from}→${remap.to}`} data-import-swap={remap.from} style={{ ...S.swapRow, ...(used ? S.swapRowUsed : null), ...(compact ? { flexDirection: 'column', alignItems: 'stretch', gap: 8 } : null) }}>
              <div style={S.swapPaths}>
                <span style={{ color: 'var(--text-muted)' }}>{remap.from}</span>
                {!compact && <I.Arrow size={16} color="var(--text-muted)" />}
                <span>{compact ? `→ ${remap.to}` : remap.to}</span>
              </div>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10 }}>
                <span style={S.gain}>+{remap.gain.toLocaleString('en-GB')} found</span>
                {used ? (
                  <button type="button" aria-label={`Stop using ${remap.to}`} aria-pressed="true" style={{ ...S.usedBtn, ...(compact ? { minHeight: 40 } : null) }} disabled={state.busy} onClick={() => state.toggleRemap(remap)}>
                    <I.Check size={14} />Used
                  </button>
                ) : (
                  <button type="button" aria-label={`Use ${remap.to}`} style={{ ...S.useBtn, ...(compact ? { minHeight: 40 } : null) }} disabled={state.busy} onClick={() => state.toggleRemap(remap)}>Use</button>
                )}
              </div>
            </div>
          ))}
        </section>
      )}

      {nothingMatched && (
        <div style={S.nothing}>
          <I.NotFound size={18} color="var(--warning, #e3b05f)" style={{ flexShrink: 0, marginTop: 2 }} />
          <div>
            <div style={{ fontWeight: 700 }}>None of these tracks are in your libraries</div>
            <div style={S.muted}>If the music is on this server under another folder, use one of the folder suggestions above. Otherwise add its folder to a library and scan it first.</div>
          </div>
        </div>
      )}

      {ambiguous.length > 0 && (
        <section aria-label="Choose one" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
            <div style={S.sectionLabel}>Choose one · {ambiguous.length.toLocaleString('en-GB')}</div>
            {!compact && <div style={S.hint}>Lines left unchosen are skipped.</div>}
          </div>
          {ambiguous.map((entry) => {
            const group = `${nameId}-line-${entry.line}`;
            const chosen = state.choices[entry.line] ?? null;
            return (
              <fieldset key={entry.line} data-import-choice={entry.line} style={S.choice}>
                <legend style={S.legend}>{compact ? entry.label : `Line ${entry.line} · ${entry.label}`}</legend>
                {(entry.candidates ?? []).map((candidate) => (
                  <label key={candidate.trackId} style={{ ...S.option, ...(compact ? { minHeight: 44 } : null) }}>
                    <input type="radio" name={group} checked={chosen === candidate.trackId} onChange={() => state.choose(entry.line, candidate.trackId)} style={S.radio} />
                    <span style={{ flex: 1, minWidth: 0 }}>
                      {compact ? candidateDetail(candidate) || candidate.title : candidate.title}
                      {!compact && candidateDetail(candidate) && <>{' · '}<span style={{ color: 'var(--text-muted)' }}>{candidateDetail(candidate)}</span></>}
                    </span>
                  </label>
                ))}
                <label style={{ ...S.option, color: 'var(--text-muted)', ...(compact ? { minHeight: 44 } : null) }}>
                  <input type="radio" name={group} checked={chosen === null} onChange={() => state.choose(entry.line, null)} style={S.radio} />
                  <span>Skip this line</span>
                </label>
              </fieldset>
            );
          })}
          {compact && <div style={S.hint}>Lines left unchosen are skipped.</div>}
        </section>
      )}

      {missing.length > 0 && (compact ? (
        <div style={S.missingCompact}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            <div style={{ fontSize: 14, fontWeight: 700 }}>Not found · {missing.length.toLocaleString('en-GB')}</div>
            <div style={S.hint}>Web links and repeats are left out too.</div>
          </div>
          <button type="button" style={{ ...S.secondaryBtn, minHeight: 44 }} onClick={state.downloadNotFound}><I.Download size={16} />Download the list</button>
        </div>
      ) : (
        <section aria-label="Not found" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
            <div style={S.sectionLabel}>Not found · {missing.length.toLocaleString('en-GB')}</div>
            <button type="button" style={S.secondaryBtn} onClick={state.downloadNotFound}><I.Download size={14} />Download the list</button>
          </div>
          <div style={S.missingList} data-import-missing="">
            {missing.slice(0, 500).map((entry) => (
              <div key={entry.line} style={S.missingRow}>
                <span style={{ color: 'var(--text-muted)', width: 72, flexShrink: 0 }}>Line {entry.line}</span>
                <span style={S.missingRaw} title={entry.raw}>{entry.raw}</span>
              </div>
            ))}
          </div>
        </section>
      ))}

      {!compact && totals && (totals.remote > 0 || totals.duplicate > 0) && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          {totals.remote > 0 && (
            <div style={S.note}><I.WebLink size={16} color="var(--accent)" style={S.noteIcon} /><span>Web links aren&apos;t imported. BoogieBox plays only music in your libraries.</span></div>
          )}
          {totals.duplicate > 0 && (
            <div style={S.note}><I.Repeat size={16} color="var(--accent)" style={S.noteIcon} /><span>A playlist holds each track once, so repeated lines are added the first time only.</span></div>
          )}
        </div>
      )}

      {state.error && <div role="alert" style={S.fieldError}>{state.error}</div>}
    </div>
  );
}

function FileLine({ state }: { state: PlaylistImportState }) {
  return (
    <div style={S.fileLine}>
      <I.File size={18} color="var(--accent)" style={{ opacity: 0.85, flexShrink: 0 }} />
      <span style={S.fileName}>{state.fileName}</span>
      {state.lineCount > 0 && <span style={{ flexShrink: 0 }}>· {state.lineCount.toLocaleString('en-GB')} {state.lineCount === 1 ? 'line' : 'lines'}</span>}
    </div>
  );
}

/** "197 tracks will be added, in the file's order". */
export function ImportWillAdd({ state, center = false }: { state: PlaylistImportState; center?: boolean }) {
  if (state.phase !== 'ready') return <span />;
  return (
    <div role="status" style={{ fontSize: 14, color: 'var(--text-muted)', ...(center ? { textAlign: 'center' } : null) }}>
      <span style={{ color: 'var(--text)', fontWeight: 700 }}>{trackCount(state.includedCount)}</span> will be added, in the file&apos;s order
    </div>
  );
}

export function CreatePlaylistButton({ state, onCreated, style }: {
  state: PlaylistImportState;
  onCreated: (result: ImportedPlaylist) => void;
  style?: React.CSSProperties;
}) {
  return (
    <button
      type="button"
      style={{ ...S.createBtn, ...style, ...(state.canCreate ? null : hybridControlStyles.disabled) }}
      disabled={!state.canCreate}
      onClick={async () => {
        const result = await state.create();
        if (result) onCreated(result);
      }}
    >
      <I.Playlist size={16} />Create playlist
    </button>
  );
}

// ─── Desktop dialog ───────────────────────────────────────────────────────────

export function PlaylistImportDialog({ file, onClose, onImported }: {
  file: File;
  onClose: () => void;
  onImported: (result: ImportedPlaylist) => void;
}) {
  const state = usePlaylistImport(file);
  const closeRef = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(state.busy);
  useEffect(() => { busyRef.current = state.busy; }, [state.busy]);

  useEffect(() => {
    const trigger = document.activeElement as HTMLElement | null;
    closeRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busyRef.current) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      if (trigger?.isConnected) trigger.focus();
    };
  }, [onClose]);

  return (
    <div role="presentation" style={S.overlay} onClick={(event) => { if (event.target === event.currentTarget && !state.busy) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="import-title" style={S.card}>
        <div style={S.header}>
          <div>
            <div style={S.eyebrow}>Import</div>
            <div id="import-title" style={S.title}>Import playlist</div>
          </div>
          <button ref={closeRef} type="button" aria-label="Close import" style={S.closeBtn} onClick={onClose}><I.Close size={16} /></button>
        </div>
        <div style={S.body}>
          <PlaylistImportBody state={state} />
        </div>
        <div style={S.footer}>
          <ImportWillAdd state={state} />
          <div style={{ display: 'flex', gap: 10 }}>
            <button type="button" style={S.cancelBtn} onClick={onClose}>Cancel</button>
            {state.phase !== 'problem' && <CreatePlaylistButton state={state} onCreated={onImported} />}
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Toast ────────────────────────────────────────────────────────────────────

export function PlaylistImportToast({ added, leftOut, onDismiss, mobile = false }: {
  added: number;
  leftOut: number;
  onDismiss: () => void;
  mobile?: boolean;
}) {
  const dismissRef = useRef(onDismiss);
  useEffect(() => { dismissRef.current = onDismiss; }, [onDismiss]);
  useEffect(() => {
    const timer = window.setTimeout(() => dismissRef.current(), 8000);
    return () => window.clearTimeout(timer);
  }, []);
  return (
    <div role="status" style={{ ...S.toast, ...(mobile ? S.toastMobile : null) }}>
      <I.Found size={20} color="var(--success, #65c590)" style={{ flexShrink: 0, marginTop: 1 }} />
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3, fontSize: 14, lineHeight: 1.5 }}>
        <div style={{ fontWeight: 700 }}>Playlist imported</div>
        <div style={{ color: 'var(--text-muted)' }}>{importSummary(added, leftOut)}</div>
      </div>
      <button type="button" aria-label="Dismiss" style={S.toastClose} onClick={onDismiss}><I.Close size={14} /></button>
    </div>
  );
}

const mono = "'IBM Plex Mono', monospace";
const raised = 'var(--surface-raised, var(--bg))';

const S: Record<string, React.CSSProperties> = {
  overlay: {
    position: 'fixed', inset: 0, zIndex: 2000, background: 'rgba(0,0,0,0.66)',
    display: 'flex', alignItems: 'flex-start', justifyContent: 'center', padding: 24, overflowY: 'auto',
  },
  card: {
    width: '100%', maxWidth: 780, background: 'var(--surface)', border: '1px solid var(--border)',
    borderRadius: 14, overflow: 'hidden', display: 'flex', flexDirection: 'column', color: 'var(--text)',
  },
  header: { padding: '20px 24px 6px', display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12 },
  eyebrow: { fontSize: 11, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: 'var(--accent)', marginBottom: 4 },
  title: { fontSize: 19, fontWeight: 750 },
  closeBtn: { ...hybridControlStyles.iconButton, width: 36, minWidth: 36, height: 36 },
  body: { padding: '14px 24px 18px' },
  stack: { display: 'flex', flexDirection: 'column', gap: 18 },
  fileLine: { display: 'flex', alignItems: 'center', gap: 10, fontSize: 14, color: 'var(--text-muted)', minWidth: 0 },
  fileName: { fontFamily: mono, fontSize: 13, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  fieldLabel: { fontSize: 13, fontWeight: 650, color: 'var(--text-muted)' },
  input: {
    minHeight: 40, padding: '8px 12px', borderRadius: 8, border: '1px solid var(--border-strong, var(--border))',
    background: 'var(--bg)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 15,
  },
  fieldError: { fontSize: 13, color: 'var(--danger, #f07b76)' },
  muted: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.55 },
  hint: { fontSize: 12, color: 'var(--text-muted)' },
  tiles: { display: 'grid', gap: 10 },
  tile: { display: 'flex', flexDirection: 'column', gap: 6, padding: 12, borderRadius: 10, background: raised, border: '1px solid transparent' },
  tileAttention: { borderColor: 'color-mix(in srgb, var(--warning, #e3b05f) 45%, transparent)' },
  tileIcon: { color: 'var(--accent)', opacity: 0.85, display: 'inline-flex' },
  tileValue: { fontSize: 20, fontWeight: 750, fontVariantNumeric: 'tabular-nums' },
  tileLabel: { fontSize: 12, color: 'var(--text-muted)' },
  swapBox: {
    display: 'flex', flexDirection: 'column', gap: 12, padding: 14, borderRadius: 12,
    border: '1px solid var(--border-strong, var(--border))', background: raised,
  },
  swapRow: {
    display: 'flex', alignItems: 'center', gap: 10, padding: '10px 12px', borderRadius: 10,
    background: 'var(--surface)', border: '1px solid var(--border)',
  },
  swapRowUsed: { borderColor: 'color-mix(in srgb, var(--success, #65c590) 50%, transparent)' },
  swapPaths: { flex: 1, minWidth: 0, display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', fontFamily: mono, fontSize: 13, wordBreak: 'break-all' },
  gain: { fontSize: 13, color: 'var(--success, #65c590)', fontWeight: 650, whiteSpace: 'nowrap' },
  useBtn: {
    minHeight: 34, padding: '6px 14px', borderRadius: 8, border: '1px solid transparent', background: 'var(--accent)',
    color: 'var(--on-accent)', fontFamily: 'inherit', fontSize: 13, fontWeight: 700, cursor: 'pointer',
  },
  usedBtn: {
    minHeight: 34, padding: '6px 12px', borderRadius: 8, border: '1px solid var(--success, #65c590)',
    background: 'color-mix(in srgb, var(--success, #65c590) 12%, transparent)', color: 'var(--success, #65c590)',
    fontFamily: 'inherit', fontSize: 13, fontWeight: 650, display: 'inline-flex', alignItems: 'center', gap: 6, cursor: 'pointer',
  },
  nothing: {
    display: 'flex', gap: 12, alignItems: 'flex-start', padding: 14, borderRadius: 12, fontSize: 14, lineHeight: 1.55,
    background: raised, border: '1px solid var(--border)',
  },
  sectionLabel: { fontSize: 13, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 0.8 },
  choice: {
    margin: 0, padding: '12px 14px', borderRadius: 10, border: '1px solid var(--border)', background: raised,
    display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0,
  },
  legend: { padding: '0 4px', fontSize: 14, fontWeight: 700 },
  option: { display: 'flex', alignItems: 'center', gap: 10, fontSize: 14, minHeight: 32, cursor: 'pointer' },
  radio: { accentColor: 'var(--accent)', width: 16, height: 16, flexShrink: 0 },
  secondaryBtn: {
    ...hybridControlStyles.secondaryButton, minHeight: 34, fontSize: 13, display: 'inline-flex', alignItems: 'center', gap: 6, flexShrink: 0,
  },
  missingList: { maxHeight: 180, overflow: 'auto', borderRadius: 10, border: '1px solid var(--border)', background: 'var(--bg)' },
  missingRow: { display: 'flex', gap: 12, padding: '8px 12px', borderBottom: '1px solid var(--border)', fontSize: 13 },
  missingRaw: { fontFamily: mono, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  missingCompact: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, padding: 12, borderRadius: 12, background: raised },
  note: { display: 'flex', gap: 8, alignItems: 'flex-start', fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.55 },
  noteIcon: { flexShrink: 0, marginTop: 2, opacity: 0.85 },
  problem: {
    display: 'flex', gap: 12, alignItems: 'flex-start', padding: '14px 16px', borderRadius: 12, fontSize: 14, lineHeight: 1.55,
    background: 'color-mix(in srgb, var(--danger, #f07b76) 12%, transparent)',
  },
  problemBody: { color: 'var(--text-muted)' },
  footer: {
    padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', alignItems: 'center',
    justifyContent: 'space-between', gap: 10, flexWrap: 'wrap',
  },
  cancelBtn: {
    minHeight: 40, padding: '8px 18px', border: '1px solid var(--border)', borderRadius: 8, background: 'var(--bg)',
    color: 'var(--text)', fontSize: 15, fontFamily: 'inherit', cursor: 'pointer',
  },
  createBtn: {
    minHeight: 40, padding: '8px 18px', border: 'none', borderRadius: 8, background: 'var(--accent)', color: 'var(--on-accent)',
    fontSize: 15, fontWeight: 700, fontFamily: 'inherit', cursor: 'pointer', display: 'inline-flex', alignItems: 'center',
    justifyContent: 'center', gap: 8,
  },
  toast: {
    position: 'fixed', right: 28, bottom: 24, zIndex: 2100, width: 'min(440px, calc(100vw - 32px))',
    display: 'flex', gap: 12, alignItems: 'flex-start', padding: '14px 16px', borderRadius: 12,
    background: 'var(--surface)', border: '1px solid var(--border-strong, var(--border))', color: 'var(--text)',
    boxShadow: '0 8px 28px rgba(0,0,0,0.45)', boxSizing: 'border-box',
  },
  toastMobile: { left: 16, right: 16, width: 'auto', bottom: 'calc(env(safe-area-inset-bottom, 0px) + 88px)' },
  toastClose: {
    width: 32, height: 32, borderRadius: 8, border: 'none', background: 'transparent', color: 'var(--text-muted)',
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer', flexShrink: 0,
  },
};
