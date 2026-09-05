/**
 * Provides progressively disclosed, touch-safe mobile administration controls.
 */

import React, { useCallback, useState } from 'react';
import { api } from '../../api';
import type {
  AdminQueueSnapshot,
  BoogieMixDeepAnalysisStatus,
  Library,
} from '../../types';

type AdminSnapshot = {
  libraries: Library[];
  queues: AdminQueueSnapshot | null;
  deepAnalysis: BoogieMixDeepAnalysisStatus | null;
};

const EMPTY_SNAPSHOT: AdminSnapshot = {
  libraries: [],
  queues: null,
  deepAnalysis: null,
};

const BACKGROUND_MODES = [
  { value: 'off', label: 'Off' },
  { value: 'playlists_only', label: 'Playlists only' },
  { value: 'favorites_and_playlists', label: 'Favorites + playlists' },
  { value: 'all_music', label: 'All music' },
] as const;

function queueCount(snapshot: AdminQueueSnapshot | null) {
  if (!snapshot) return 0;
  return Object.values(snapshot.queues).reduce((total, queue) => total + queue.length, 0);
}

/** Mobile Admin Settings Panel is part of this module's public API. */
export default function MobileAdminSettingsPanel() {
  const [expanded, setExpanded] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<AdminSnapshot>(EMPTY_SNAPSHOT);
  const [selectedLibraryId, setSelectedLibraryId] = useState('');
  const [error, setError] = useState('');
  const [actionStatus, setActionStatus] = useState('');

  const loadSnapshot = useCallback(async () => {
    setLoading(true);
    setError('');
    const results = await Promise.allSettled([
      api.libraries.list(),
      api.admin.queues(),
      api.boogiemix.deepAnalysisStatus(),
    ]);
    setSnapshot((current) => ({
      libraries: results[0].status === 'fulfilled' ? results[0].value : current.libraries,
      queues: results[1].status === 'fulfilled' ? results[1].value : current.queues,
      deepAnalysis: results[2].status === 'fulfilled' ? results[2].value : current.deepAnalysis,
    }));
    const failed = results.filter((result) => result.status === 'rejected').length;
    setError(failed ? `Could not load ${failed} admin status ${failed === 1 ? 'source' : 'sources'}.` : '');
    setLoaded(true);
    setLoading(false);
  }, []);

  const runAction = async (key: string, action: () => Promise<string>) => {
    setBusyKey(key);
    setActionStatus('');
    setError('');
    try {
      setActionStatus(await action());
      await loadSnapshot();
    } catch (actionError: any) {
      setError(actionError?.message || 'Could not complete the admin action.');
    } finally {
      setBusyKey(null);
    }
  };

  const deepStatus = snapshot.deepAnalysis;
  const backgroundMode = deepStatus?.controls?.backgroundMode ?? 'off';
  const backgroundPaused = deepStatus?.controls?.pauseBackground ?? false;
  const totalQueued = queueCount(snapshot.queues);

  return (
    <section aria-labelledby="mobile-settings-admin" style={styles.section}>
      <div style={styles.header}>
        <div>
          <div style={styles.eyebrow}>Administrator</div>
          <h3 id="mobile-settings-admin" style={styles.heading}>Server operations</h3>
        </div>
        <span style={styles.adminBadge}>Admin</span>
      </div>
      <p style={styles.description}>
        Check libraries, background work, and BoogieMix health without leaving mobile.
      </p>
      <button
        type="button"
        aria-expanded={expanded}
        style={styles.disclosureButton}
        onClick={() => {
          const next = !expanded;
          setExpanded(next);
          if (next && !loaded) void loadSnapshot();
        }}
      >
        <span>{expanded ? 'Hide admin tools' : 'Open admin tools'}</span>
        <span aria-hidden="true">{expanded ? '−' : '+'}</span>
      </button>

      {expanded ? (
        <div style={styles.panel}>
          <div style={styles.panelToolbar}>
            <span style={styles.panelTitle}>Live status</span>
            <button
              type="button"
              disabled={loading}
              style={styles.refreshButton}
              onClick={() => void loadSnapshot()}
            >
              {loading ? 'Refreshing…' : 'Refresh'}
            </button>
          </div>

          {loading && !loaded ? <div role="status" style={styles.feedback}>Loading admin status…</div> : null}
          {actionStatus ? <div role="status" style={styles.success}>{actionStatus}</div> : null}
          {error ? <div role="alert" style={styles.error}>{error}</div> : null}

          {loaded ? (
            <>
              <div style={styles.metrics}>
                <div style={styles.metric}>
                  <strong style={styles.metricValue}>{snapshot.libraries.length}</strong>
                  <span>Libraries</span>
                </div>
                <div style={styles.metric}>
                  <strong style={styles.metricValue}>{totalQueued}</strong>
                  <span>Queued jobs</span>
                </div>
                <div style={styles.metric}>
                  <strong style={styles.metricValue}>{deepStatus?.cache.analyzedTracks ?? '—'}</strong>
                  <span>Analyzed</span>
                </div>
              </div>

              <div style={styles.subsection}>
                <div style={styles.subsectionHeader}>
                  <div>
                    <div style={styles.subsectionTitle}>Libraries</div>
                    <div style={styles.subsectionCopy}>Start an incremental scan for one source.</div>
                  </div>
                </div>
                {snapshot.libraries.length ? (
                  <div style={styles.libraryList}>
                    {snapshot.libraries.map((library) => {
                      const key = `scan-${String(library.id)}`;
                      return (
                        <div key={String(library.id)} style={styles.libraryRow}>
                          <div style={styles.libraryCopy}>
                            <strong style={styles.libraryName}>{library.name}</strong>
                            <span style={styles.libraryMeta}>
                              {library.track_count.toLocaleString()} tracks
                              {library.folder_count ? ` · ${library.folder_count} folders` : ''}
                            </span>
                            <span style={styles.libraryPath}>
                              {library.primary_path || library.path || 'Library path unavailable'}
                            </span>
                          </div>
                          <button
                            type="button"
                            disabled={busyKey !== null}
                            style={styles.actionButton}
                            onClick={() => void runAction(key, async () => {
                              await api.libraries.scan(library.id);
                              return `Scan queued for ${library.name}.`;
                            })}
                          >
                            {busyKey === key ? 'Queueing…' : 'Scan'}
                          </button>
                        </div>
                      );
                    })}
                  </div>
                ) : <div style={styles.empty}>No libraries configured.</div>}
              </div>

              <div style={styles.subsection}>
                <div style={styles.subsectionTitle}>Work queues</div>
                <div style={styles.queueGrid}>
                  {[
                    ['Scans', snapshot.queues?.queues.scan.length ?? 0],
                    ['Post-scan', snapshot.queues?.queues.postScan.length ?? 0],
                    ['Mixes', snapshot.queues?.queues.mix.length ?? 0],
                    ['Deep analysis', snapshot.queues?.queues.deepAnalysis.length ?? 0],
                  ].map(([label, count]) => (
                    <div key={String(label)} style={styles.queueItem}>
                      <span>{label}</span>
                      <strong style={styles.queueCount}>{count}</strong>
                    </div>
                  ))}
                </div>
              </div>

              <div style={styles.subsection}>
                <div style={styles.runtimeHeader}>
                  <div>
                    <div style={styles.subsectionTitle}>BoogieMix analysis</div>
                    <div style={styles.subsectionCopy}>
                      {deepStatus?.runtime?.summary || 'Runtime status unavailable.'}
                    </div>
                  </div>
                  <span style={{
                    ...styles.runtimeBadge,
                    ...(deepStatus?.runtime?.enabled ? styles.runtimeBadgeReady : null),
                  }}>
                    {deepStatus?.runtime?.enabled ? 'Ready' : 'Limited'}
                  </span>
                </div>
                {deepStatus ? (
                  <>
                    <div style={styles.deepQueue}>
                      <span>{deepStatus.queue.pending} pending</span>
                      <span>{deepStatus.queue.running} running</span>
                      <span>{deepStatus.queue.failed} failed</span>
                    </div>
                    <label style={styles.field}>
                      <span style={styles.fieldLabel}>Background analysis</span>
                      <select
                        aria-label="Background analysis"
                        value={backgroundMode}
                        disabled={busyKey !== null}
                        style={styles.select}
                        onChange={(event) => {
                          const value = event.target.value;
                          void runAction('background-mode', async () => {
                            await api.settings.update({ boogiemixDeepAnalysisBackgroundMode: value });
                            return 'Background analysis mode saved.';
                          });
                        }}
                      >
                        {BACKGROUND_MODES.map((mode) => (
                          <option key={mode.value} value={mode.value}>{mode.label}</option>
                        ))}
                      </select>
                    </label>
                    {backgroundMode === 'all_music' ? (
                      <div style={styles.warning}>
                        All music can create sustained CPU and disk activity.
                      </div>
                    ) : null}
                    <label style={styles.field}>
                      <span style={styles.fieldLabel}>Analyze a library</span>
                      <span style={styles.inlineActions}>
                        <select
                          aria-label="Deep-analysis library"
                          value={selectedLibraryId}
                          onChange={(event) => setSelectedLibraryId(event.target.value)}
                          disabled={busyKey !== null}
                          style={styles.select}
                        >
                          <option value="">Choose library</option>
                          {snapshot.libraries.map((library) => (
                            <option key={String(library.id)} value={String(library.id)}>{library.name}</option>
                          ))}
                        </select>
                        <button
                          type="button"
                          disabled={busyKey !== null || !snapshot.libraries.length}
                          style={styles.actionButton}
                          onClick={() => {
                            const library = snapshot.libraries.find(
                              (item) => String(item.id) === selectedLibraryId,
                            );
                            if (!selectedLibraryId || !library) {
                              setError('Choose a library to analyze.');
                              return;
                            }
                            void runAction('analyze-library', async () => {
                              const result = await api.boogiemix.queueLibraryDeepAnalysis(library.id);
                              return `Queued ${result.queued} tracks from ${library.name}.`;
                            });
                          }}
                        >
                          {busyKey === 'analyze-library' ? 'Queueing…' : 'Analyze'}
                        </button>
                      </span>
                    </label>
                    <button
                      type="button"
                      disabled={busyKey !== null}
                      style={styles.secondaryButton}
                      onClick={() => void runAction('pause-background', async () => {
                        if (backgroundPaused) {
                          await api.boogiemix.resumeDeepAnalysisBackground();
                          return 'Background analysis resumed.';
                        }
                        await api.boogiemix.pauseDeepAnalysisBackground();
                        return 'Background analysis paused.';
                      })}
                    >
                      {busyKey === 'pause-background'
                        ? 'Updating…'
                        : backgroundPaused ? 'Resume background analysis' : 'Pause background analysis'}
                    </button>
                  </>
                ) : null}
              </div>
            </>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

const styles: Record<string, React.CSSProperties> = {
  section: {
    display: 'grid',
    gap: 10,
    padding: 12,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'var(--surface)',
  },
  header: {
    minHeight: 38,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
  },
  eyebrow: {
    color: 'var(--text-faint)',
    fontSize: 11,
    fontWeight: 750,
    letterSpacing: 0.7,
    textTransform: 'uppercase',
  },
  heading: {
    margin: '3px 0 0',
    color: 'var(--text)',
    fontSize: 19,
    fontWeight: 800,
    letterSpacing: -0.25,
  },
  adminBadge: {
    padding: '4px 7px',
    borderRadius: 999,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 11,
    fontWeight: 750,
  },
  description: {
    margin: '-2px 0 0',
    color: 'var(--text-muted)',
    fontSize: 12,
    lineHeight: 1.5,
  },
  disclosureButton: {
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '0 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  panel: {
    display: 'grid',
    gap: 10,
    paddingTop: 2,
  },
  panelToolbar: {
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
  },
  panelTitle: { color: 'var(--text)', fontSize: 14, fontWeight: 800 },
  refreshButton: {
    minHeight: 44,
    padding: '0 12px',
    border: 'none',
    borderRadius: 10,
    background: 'transparent',
    color: 'var(--accent)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  feedback: {
    padding: '10px 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 12,
  },
  success: {
    padding: '10px 12px',
    border: '1px solid color-mix(in srgb, var(--success) 32%, var(--divider-subtle))',
    borderRadius: 11,
    background: 'color-mix(in srgb, var(--success) 9%, var(--surface))',
    color: 'var(--success)',
    fontSize: 12,
    fontWeight: 650,
  },
  error: {
    padding: '10px 12px',
    border: '1px solid color-mix(in srgb, var(--danger) 32%, var(--divider-subtle))',
    borderRadius: 11,
    background: 'color-mix(in srgb, var(--danger) 8%, var(--surface))',
    color: 'var(--danger)',
    fontSize: 12,
    fontWeight: 650,
  },
  metrics: {
    display: 'grid',
    gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
    gap: 6,
  },
  metric: {
    minWidth: 0,
    display: 'grid',
    gap: 3,
    padding: '10px 8px',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 11,
    textAlign: 'center',
  },
  metricValue: { color: 'var(--text)', fontSize: 18, fontWeight: 800 },
  subsection: {
    display: 'grid',
    gap: 9,
    paddingTop: 12,
    borderTop: '1px solid var(--divider-subtle)',
  },
  subsectionHeader: {
    display: 'flex',
    alignItems: 'start',
    justifyContent: 'space-between',
    gap: 10,
  },
  subsectionTitle: { color: 'var(--text)', fontSize: 14, fontWeight: 800 },
  subsectionCopy: {
    marginTop: 3,
    color: 'var(--text-muted)',
    fontSize: 12,
    lineHeight: 1.45,
  },
  libraryList: { display: 'grid', gap: 7 },
  libraryRow: {
    minHeight: 68,
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) auto',
    alignItems: 'center',
    gap: 10,
    padding: '8px 8px 8px 10px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 12,
    background: 'var(--surface-subtle)',
  },
  libraryCopy: { minWidth: 0, display: 'grid', gap: 2 },
  libraryName: {
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 13,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  libraryMeta: { color: 'var(--text-muted)', fontSize: 11 },
  libraryPath: {
    overflow: 'hidden',
    color: 'var(--text-faint)',
    fontSize: 10,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  actionButton: {
    minHeight: 44,
    padding: '0 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 10,
    background: 'var(--surface-raised)',
    color: 'var(--accent)',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 800,
  },
  empty: {
    padding: '10px 12px',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 12,
  },
  queueGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 6,
  },
  queueItem: {
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 8,
    padding: '0 10px',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 11,
  },
  queueCount: { color: 'var(--text)', fontSize: 14 },
  runtimeHeader: {
    display: 'flex',
    alignItems: 'start',
    justifyContent: 'space-between',
    gap: 10,
  },
  runtimeBadge: {
    flexShrink: 0,
    padding: '4px 7px',
    borderRadius: 999,
    background: 'color-mix(in srgb, var(--warning) 10%, var(--surface))',
    color: 'var(--warning)',
    fontSize: 11,
    fontWeight: 750,
  },
  runtimeBadgeReady: {
    background: 'color-mix(in srgb, var(--success) 10%, var(--surface))',
    color: 'var(--success)',
  },
  deepQueue: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: '5px 12px',
    color: 'var(--text-muted)',
    fontSize: 11,
  },
  field: { display: 'grid', gap: 5 },
  fieldLabel: { color: 'var(--text)', fontSize: 12, fontWeight: 750 },
  select: {
    width: '100%',
    minWidth: 0,
    minHeight: 44,
    padding: '0 10px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 700,
  },
  inlineActions: {
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) auto',
    gap: 7,
  },
  warning: {
    padding: '9px 10px',
    borderRadius: 10,
    background: 'color-mix(in srgb, var(--warning) 9%, var(--surface))',
    color: 'var(--warning)',
    fontSize: 11,
    fontWeight: 650,
    lineHeight: 1.4,
  },
  secondaryButton: {
    width: '100%',
    minHeight: 44,
    padding: '0 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 10,
    background: 'transparent',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: 750,
  },
};
