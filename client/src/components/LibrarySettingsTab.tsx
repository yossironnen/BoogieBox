/**
 * Defines the Library Settings Tab React component and related UI helpers.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import FolderPickerModal from './FolderPickerModal';
import ConfirmModal from './ConfirmModal';
import { platform } from '../platform';
import type { ClientEntityId, Library, LibraryFolder, ScanJob } from '../types';
import { parseServerDate } from '../utils';

type Props = {
  libraries: Library[];
  onRefresh?: () => Promise<void>;
};

type PathTestResult = {
  exists: boolean;
  isDirectory: boolean;
  displayName?: string;
  normalized?: string;
  error?: string;
};

export function formatPendingStatus(job: ScanJob): string {
  const queuePos = job.queue_position;
  const running = job.running_job;
  const startedAtMs = Date.parse(job.started_at || '');
  const queuedMinutes = Number.isFinite(startedAtMs)
    ? Math.max(0, Math.floor((Date.now() - startedAtMs) / 60000))
    : 0;

  if (running) {
    const posText = queuePos && queuePos > 0 ? `Queue #${queuePos}` : 'Queued';
    return `${posText} - Running now: ${running.library_name} (job #${running.id})`;
  }
  if (queuedMinutes >= 1) {
    return `Queued - waiting for scan worker (${queuedMinutes}m)`;
  }
  return 'Queued...';
}

const Icon = {
  Scan: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="23 4 23 10 17 10" />
      <polyline points="1 20 1 14 7 14" />
      <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
    </svg>
  ),
  Plus: () => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  ),
  Trash: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14H6L5 6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
      <path d="M9 6V4h6v2" />
    </svg>
  ),
};

const L = {
  sectionTitle: { fontSize: 16, fontWeight: 700, marginBottom: 8, color: 'var(--text)' },
  intro: { color: 'var(--text-muted)', fontSize: 12, lineHeight: 1.6, margin: '0 0 20px' },
  addForm: { display: 'flex', gap: 8, flexWrap: 'wrap' as const, alignItems: 'center' as const },
  input: {
    flex: 1,
    minWidth: 220,
    backgroundColor: 'var(--bg)',
    border: '1px solid var(--border)',
    color: 'var(--text)',
    borderRadius: 6,
    padding: '8px 12px',
    fontSize: 13,
    fontFamily: 'inherit',
    outline: 'none',
  },
  btnPrimary: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    backgroundColor: 'var(--accent)',
    color: '#fff',
    border: 'none',
    borderRadius: 6,
    padding: '8px 16px',
    cursor: 'pointer',
    fontSize: 13,
    fontFamily: 'inherit',
    fontWeight: 600,
  },
  btnSecondary: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    backgroundColor: 'var(--bg)',
    color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    padding: '6px 12px',
    cursor: 'pointer',
    fontSize: 12,
    fontFamily: 'inherit',
  },
  btnDanger: {
    display: 'flex',
    alignItems: 'center',
    backgroundColor: 'transparent',
    color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    padding: '6px 8px',
    cursor: 'pointer',
  },
  errorMsg: { color: '#ef4444', marginTop: 8, fontSize: 12 },
  helperCard: {
    marginTop: 8,
    padding: '10px 14px',
    borderRadius: 6,
    fontSize: 12,
  },
  muted: { color: 'var(--text-muted)' },
  listWrap: { marginTop: 20 },
  libCard: {
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 8,
    padding: '14px 16px',
    marginBottom: 10,
  },
  libCardHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'flex-start',
    gap: 12,
  },
  libName: { fontWeight: 700, color: 'var(--text)', fontSize: 14, marginBottom: 4 },
  libPath: { color: 'var(--text)', fontSize: 12, wordBreak: 'break-all' as const, marginBottom: 4 },
  folderList: { display: 'flex', flexDirection: 'column' as const, gap: 6, margin: '10px 0' },
  folderRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
    padding: '8px 10px',
    borderRadius: 6,
    backgroundColor: 'var(--bg)',
    border: '1px solid var(--border)',
  },
  folderPathText: { color: 'var(--text)', fontSize: 12, wordBreak: 'break-all' as const, flex: 1 },
  libMeta: { color: 'var(--text-muted)', fontSize: 11 },
  libActions: { display: 'flex', gap: 8 },
  progressWrap: { marginTop: 10 },
  progressBar: {
    height: 8,
    backgroundColor: 'var(--bg)',
    borderRadius: 999,
    overflow: 'hidden' as const,
    border: '1px solid var(--border)',
  },
  progressFill: {
    height: '100%',
    borderRadius: 999,
    transition: 'width 0.2s ease',
  },
  progressText: { fontSize: 11, color: 'var(--text-muted)', marginTop: 4 },
};

/** Library Settings Tab is part of this module's public API. */
export default function LibrarySettingsTab({ libraries, onRefresh }: Props) {
  const [newPath, setNewPath] = useState('');
  const [pendingFolders, setPendingFolders] = useState<string[]>([]);
  const [newName, setNewName] = useState('');
  const [error, setError] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerInitial, setPickerInitial] = useState<string | undefined>(undefined);
  const pickerResolveRef = useRef<((path: string | null) => void) | null>(null);
  const [editingLibraryId, setEditingLibraryId] = useState<ClientEntityId | null>(null);
  const [editingName, setEditingName] = useState('');
  const [renameBusy, setRenameBusy] = useState(false);
  const [testResult, setTestResult] = useState<PathTestResult | null>(null);
  const [folderDrafts, setFolderDrafts] = useState<Record<string, string>>({});
  const [activeJobs, setActiveJobs] = useState<Record<string, ScanJob>>({});
  const jobsPollBusyRef = useRef(false);
  const summaryPollBusyRef = useRef(false);
  const [pendingConfirm, setPendingConfirm] = useState<{
    title: string;
    message: string;
    confirmLabel?: string;
    onConfirm: () => void;
  } | null>(null);

  const getFolders = useCallback((library: Library): LibraryFolder[] => {
    if (library.folders?.length) return library.folders;
    const primaryPath = library.primary_path ?? library.path;
    if (!primaryPath) return [];
    return [{ id: library.id, library_id: library.id, path: primaryPath, position: 0 }];
  }, []);

  const openBrowse = useCallback((initialPath?: string): Promise<string | null> => {
    if (platform.isDesktop) {
      return platform.selectFolder(initialPath);
    }
    return new Promise((resolve) => {
      pickerResolveRef.current = resolve;
      setPickerInitial(initialPath);
      setPickerOpen(true);
    });
  }, []);

  const refreshLibraries = useCallback(async () => {
    await onRefresh?.();
  }, [onRefresh]);

  const pickVisibleJob = useCallback((jobs: ScanJob[], trackedJob?: ScanJob): ScanJob | null => {
    if (!jobs.length) return trackedJob ?? null;
    const tracked = trackedJob ? jobs.find((job) => job.id === trackedJob.id) : null;
    if (tracked) return tracked;
    const active = jobs.find((job) => job.status === 'running' || job.status === 'pending');
    if (active) return active;
    const latest = jobs[0];
    if (latest.status === 'failed' || latest.status === 'error') return latest;
    return null;
  }, []);

  const addLibrary = async () => {
    const folders = Array.from(new Set([...pendingFolders, ...(newPath.trim() ? [newPath.trim()] : [])]));
    if (!folders.length) return;
    setError('');
    setTestResult(null);
    try {
      await api.libraries.add(folders, newName.trim() || undefined);
      setNewPath('');
      setPendingFolders([]);
      setNewName('');
      await refreshLibraries();
    } catch (e: any) {
      setError(e.message || 'Failed to add library');
    }
  };

  const testPath = async () => {
    if (!newPath.trim()) return;
    setError('');
    try {
      setTestResult(await api.debugTestPath(newPath.trim()));
    } catch (e: any) {
      setError(e.message || 'Failed to test path');
    }
  };

  const queueFolder = () => {
    const nextPath = newPath.trim();
    if (!nextPath) return;
    setPendingFolders((prev) => prev.includes(nextPath) ? prev : [...prev, nextPath]);
    setNewPath('');
    setTestResult(null);
  };

  const removeLibrary = async (id: ClientEntityId) => {
    try {
      await api.libraries.remove(id);
      await refreshLibraries();
    } catch (e: any) {
      setError(e.message || 'Failed to remove library');
    }
  };

  const confirmRemoveLibrary = (id: ClientEntityId) => setPendingConfirm({
    title: 'Remove this library?',
    message: 'This also removes all of its scanned data — tracks, artwork, playlists entries, everything.',
    confirmLabel: 'Remove Library',
    onConfirm: () => removeLibrary(id),
  });

  const addFolderToLibrary = async (libraryId: ClientEntityId) => {
    const nextPath = (folderDrafts[String(libraryId)] ?? '').trim();
    if (!nextPath) return;
    try {
      setError('');
      await api.libraries.addFolder(libraryId, nextPath);
      setFolderDrafts((prev) => ({ ...prev, [String(libraryId)]: '' }));
      await refreshLibraries();
    } catch (e: any) {
      setError(e.message || 'Failed to add folder');
    }
  };

  const removeFolderFromLibrary = async (libraryId: ClientEntityId, folderId: ClientEntityId) => {
    try {
      setError('');
      await api.libraries.removeFolder(libraryId, folderId);
      await refreshLibraries();
    } catch (e: any) {
      setError(e.message || 'Failed to remove folder');
    }
  };

  const confirmRemoveFolder = (libraryId: ClientEntityId, folderId: ClientEntityId) => setPendingConfirm({
    title: 'Remove this folder from the library?',
    message: 'Tracks scanned from it will be removed from the library too.',
    confirmLabel: 'Remove Folder',
    onConfirm: () => removeFolderFromLibrary(libraryId, folderId),
  });

  const startRename = (library: Library) => {
    setError('');
    setEditingLibraryId(library.id);
    setEditingName(library.name);
  };

  const cancelRename = () => {
    setEditingLibraryId(null);
    setEditingName('');
    setRenameBusy(false);
  };

  const saveRename = async (libraryId: ClientEntityId) => {
    const nextName = editingName.trim();
    if (!nextName) {
      setError('Library name is required');
      return;
    }
    try {
      setRenameBusy(true);
      setError('');
      await api.libraries.rename(libraryId, nextName);
      cancelRename();
      await refreshLibraries();
    } catch (e: any) {
      setError(e.message || 'Failed to rename library');
      setRenameBusy(false);
    }
  };

  const startScan = async (library: Library) => {
    try {
      const { jobId } = await api.libraries.scan(library.id);
      setActiveJobs((prev) => ({
        ...prev,
        [String(library.id)]: {
          id: jobId,
          library_id: library.id,
          status: 'pending' as const,
          files_found: 0,
          files_scanned: 0,
          errors: 0,
          started_at: new Date().toISOString(),
          finished_at: null,
        },
      }));
      const poll = setInterval(async () => {
        try {
          const job = await api.scanJobs.get(jobId);
          setActiveJobs((prev) => ({ ...prev, [library.id]: job }));
          if (job.status !== 'running' && job.status !== 'pending') {
            clearInterval(poll);
            await refreshLibraries();
            setTimeout(() => {
              refreshLibraries().catch(() => {});
            }, 800);
          }
        } catch {
          // Ignore transient polling failures.
        }
      }, 1000);
    } catch (e: any) {
      setError(e.message || 'Failed to start scan');
    }
  };

  useEffect(() => {
    if (!libraries.length) {
      setActiveJobs({});
      return;
    }

    const refreshVisibleJobs = async () => {
      if (jobsPollBusyRef.current) return;
      jobsPollBusyRef.current = true;
      try {
        const activeJobRows = await api.scanJobs.active();
        const jobsByLibrary = new Map<string, ScanJob[]>();
        for (const job of activeJobRows) {
          const bucket = jobsByLibrary.get(String(job.library_id)) ?? [];
          bucket.push(job);
          jobsByLibrary.set(String(job.library_id), bucket);
        }
        setActiveJobs((prev) => {
          const next: Record<string, ScanJob> = {};
          libraries.forEach((library) => {
            const libraryKey = String(library.id);
            const selected = pickVisibleJob(jobsByLibrary.get(libraryKey) ?? [], prev[libraryKey]);
            if (selected) next[libraryKey] = selected;
          });
          return next;
        });
      } catch {
        // Ignore transient polling failures.
      } finally {
        jobsPollBusyRef.current = false;
      }
    };

    refreshVisibleJobs();
    const pollId = setInterval(refreshVisibleJobs, 3000);
    return () => clearInterval(pollId);
  }, [libraries, pickVisibleJob]);

  useEffect(() => {
    if (!libraries.length || !onRefresh) return;

    const refreshSummaries = async () => {
      if (summaryPollBusyRef.current) return;
      summaryPollBusyRef.current = true;
      try {
        await onRefresh();
      } catch {
        // Ignore transient polling failures.
      } finally {
        summaryPollBusyRef.current = false;
      }
    };

    const pollId = setInterval(refreshSummaries, 12000);
    return () => clearInterval(pollId);
  }, [libraries.length, onRefresh]);

  return (
    <>
    <div>
      <div style={L.sectionTitle}>Libraries</div>
      <p style={L.intro}>
        Manage library folders here. Build one library from several folders, validate paths before saving,
        remove folders from existing libraries, and trigger manual scans without leaving Settings.
      </p>
      <div style={L.addForm}>
        <input
          style={L.input}
          placeholder="Folder path (e.g. C:\\Music or \\\\server\\share\\music)"
          value={newPath}
          onChange={(e) => {
            setNewPath(e.target.value);
            setTestResult(null);
          }}
          onKeyDown={(e) => e.key === 'Enter' && addLibrary()}
        />
        <input
          style={{ ...L.input, maxWidth: 180 }}
          placeholder="Name (optional)"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && addLibrary()}
        />
        <button
          style={L.btnSecondary}
          type="button"
          onClick={async () => {
            try {
              const folder = await openBrowse(newPath.trim() || undefined);
              if (folder) { setNewPath(folder); setTestResult(null); }
            } catch (e: any) {
              setError(e.message || 'Folder picker failed');
            }
          }}
        >
          Browse
        </button>
        <button style={L.btnSecondary} onClick={testPath} type="button">
          Test
        </button>
        <button style={L.btnSecondary} onClick={queueFolder} type="button">
          Queue Folder
        </button>
        <button style={L.btnPrimary} onClick={addLibrary} type="button">
          <Icon.Plus /> Add
        </button>
      </div>
      {pendingFolders.length > 0 && (
        <div style={L.folderList}>
          {pendingFolders.map((folder) => (
            <div key={folder} style={L.folderRow}>
              <div style={L.folderPathText}>{folder}</div>
              <button
                style={L.btnDanger}
                type="button"
                title="Remove queued folder"
                onClick={() => setPendingFolders((prev) => prev.filter((candidate) => candidate !== folder))}
              >
                <Icon.Trash />
              </button>
            </div>
          ))}
        </div>
      )}
      {error && <div style={L.errorMsg}>{error}</div>}
      {testResult && (
        <div
          style={{
            ...L.helperCard,
            backgroundColor: testResult.exists && testResult.isDirectory ? '#052e16' : '#2d0a0a',
            border: `1px solid ${testResult.exists && testResult.isDirectory ? '#166534' : '#7f1d1d'}`,
            color: testResult.exists && testResult.isDirectory ? '#86efac' : '#fca5a5',
          }}
        >
          {testResult.exists && testResult.isDirectory
            ? `Path OK - will be named "${testResult.displayName}"`
            : testResult.exists
              ? 'Path exists but is not a directory'
              : `Path not found: ${testResult.normalized}`}
          {testResult.error && <div style={{ marginTop: 4 }}>{testResult.error}</div>}
        </div>
      )}
      <div style={L.listWrap}>
        {libraries.length === 0 && <p style={L.muted}>No libraries added yet.</p>}
        {libraries.map((library) => {
          const folders = getFolders(library);
          const primaryPath = library.primary_path ?? library.path ?? folders[0]?.path ?? '';
          const job = activeJobs[String(library.id)];
          const progress = job && job.files_found > 0
            ? `${Math.round((job.files_scanned / job.files_found) * 100)}%`
            : '0%';

          return (
            <div key={library.id} style={L.libCard}>
              <div style={L.libCardHeader}>
                <div>
                  {editingLibraryId === library.id ? (
                    <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 6, flexWrap: 'wrap' }}>
                      <input
                        aria-label={`Library name for ${primaryPath}`}
                        style={{ ...L.input, minWidth: 180, maxWidth: 260, flex: '0 1 260px', padding: '6px 10px' }}
                        value={editingName}
                        onChange={(e) => setEditingName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') void saveRename(library.id);
                          if (e.key === 'Escape') cancelRename();
                        }}
                      />
                      <button style={L.btnSecondary} type="button" onClick={() => void saveRename(library.id)} disabled={renameBusy}>
                        {renameBusy ? 'Saving...' : 'Save'}
                      </button>
                      <button style={L.btnSecondary} type="button" onClick={cancelRename} disabled={renameBusy}>
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <div style={L.libName}>{library.name}</div>
                  )}
                  <div style={L.libPath}>{primaryPath}</div>
                  <div style={L.libMeta}>
                    MUSIC
                    {' - '}
                    {library.track_count.toLocaleString()} tracks
                    {' - '}
                    {(library.folder_count ?? folders.length).toLocaleString()} folder{(library.folder_count ?? folders.length) === 1 ? '' : 's'}
                    {library.last_scan && ` - Last scan: ${(parseServerDate(library.last_scan) ?? new Date(library.last_scan)).toLocaleString()}`}
                  </div>
                  <div style={L.folderList}>
                    {folders.map((folder, index) => (
                      <div key={String(folder.id)} style={L.folderRow}>
                        <div style={L.folderPathText}>{index === 0 ? `Primary: ${folder.path}` : folder.path}</div>
                        {folders.length > 1 ? (
                          <button
                            style={L.btnDanger}
                            type="button"
                            title="Remove folder"
                            onClick={() => confirmRemoveFolder(library.id, folder.id)}
                          >
                            <Icon.Trash />
                          </button>
                        ) : null}
                      </div>
                    ))}
                  </div>
                  <div style={L.addForm}>
                    <input
                      style={L.input}
                      placeholder="Add another folder"
                      value={folderDrafts[String(library.id)] ?? ''}
                      onChange={(e) => setFolderDrafts((prev) => ({ ...prev, [String(library.id)]: e.target.value }))}
                      onKeyDown={(e) => e.key === 'Enter' && addFolderToLibrary(library.id)}
                    />
                    <button
                      style={L.btnSecondary}
                      type="button"
                      onClick={async () => {
                        try {
                          const folder = await openBrowse(
                            (folderDrafts[String(library.id)] ?? '').trim() || undefined,
                          );
                          if (folder) setFolderDrafts((prev) => ({ ...prev, [String(library.id)]: folder }));
                        } catch (e: any) {
                          setError(e.message || 'Folder picker failed');
                        }
                      }}
                    >
                      Browse
                    </button>
                    <button style={L.btnSecondary} type="button" onClick={() => addFolderToLibrary(library.id)}>
                      Add Folder
                    </button>
                  </div>
                </div>
                <div style={L.libActions}>
                  {editingLibraryId !== library.id ? (
                    <button
                      style={L.btnSecondary}
                      onClick={() => startRename(library)}
                      type="button"
                    >
                      Rename
                    </button>
                  ) : null}
                  <button
                    style={L.btnSecondary}
                    onClick={() => startScan(library)}
                    disabled={job?.status === 'running'}
                    type="button"
                  >
                    <Icon.Scan /> {job?.status === 'running' ? 'Scanning...' : 'Scan'}
                  </button>
                  <button style={L.btnDanger} onClick={() => confirmRemoveLibrary(library.id)} type="button" title="Remove library">
                    <Icon.Trash />
                  </button>
                </div>
              </div>
              {job && (
                <div style={L.progressWrap}>
                  <div style={L.progressBar}>
                    <div
                      style={{
                        ...L.progressFill,
                        width: progress,
                        backgroundColor:
                          job.status === 'failed' || job.status === 'error'
                            ? '#ef4444'
                            : job.status === 'done'
                              ? '#22c55e'
                              : 'var(--accent)',
                      }}
                    />
                  </div>
                  <div style={L.progressText}>
                    {job.status === 'done'
                      ? `Done - ${job.files_scanned.toLocaleString()} tracks, ${job.errors} errors`
                      : (job.status === 'failed' || job.status === 'error')
                        ? 'Error'
                        : job.status === 'pending'
                          ? formatPendingStatus(job)
                          : `${job.files_scanned.toLocaleString()} / ${job.files_found.toLocaleString()} files...`}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
    {pickerOpen && (
      <FolderPickerModal
        initialPath={pickerInitial}
        onSelect={(path) => {
          setPickerOpen(false);
          pickerResolveRef.current?.(path);
          pickerResolveRef.current = null;
        }}
        onClose={() => {
          setPickerOpen(false);
          pickerResolveRef.current?.(null);
          pickerResolveRef.current = null;
        }}
      />
    )}
    {pendingConfirm && (
      <ConfirmModal
        title={pendingConfirm.title}
        message={pendingConfirm.message}
        confirmLabel={pendingConfirm.confirmLabel}
        tone="danger"
        onConfirm={() => { const { onConfirm } = pendingConfirm; setPendingConfirm(null); onConfirm(); }}
        onCancel={() => setPendingConfirm(null)}
      />
    )}
    </>
  );
}
