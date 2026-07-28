/**
 * Defines the compact Hybrid BoogieMix workflow used by mobile playlist detail.
 */

import React, { useEffect, useMemo, useState } from 'react';
import { api } from '../../api';
import type {
  BoogieMixDeepAnalysisStatus,
  BoogieMixJob,
  BoogieMixOutput,
  ClientEntityId,
  PlaylistDeepAnalysisProgress,
} from '../../types';
import type { EntityId } from '../../entityId';
import { hybridMobileContentStyles } from '../../hybridPreview';

type MixStyle = 'chill_blend' | 'club_blend' | 'long_build' | 'safe_mix';
type MixQuality = 'standard' | 'high_quality';

const ACTIVE_JOB_STATUSES = ['pending', 'analyzing', 'planning', 'rendering'] as const;
const CANCELABLE_JOB_STATUSES = ['pending', 'analyzing', 'planning'] as const;

const STYLE_OPTIONS: Array<{ value: MixStyle; label: string }> = [
  { value: 'chill_blend', label: 'Chill blend' },
  { value: 'club_blend', label: 'Club blend' },
  { value: 'long_build', label: 'Long build' },
  { value: 'safe_mix', label: 'Safe mix' },
];

function isActiveJob(job: BoogieMixJob | null): boolean {
  return !!job && ACTIVE_JOB_STATUSES.includes(job.status as typeof ACTIVE_JOB_STATUSES[number]);
}

function isCancelableJob(job: BoogieMixJob | null): boolean {
  return !!job && CANCELABLE_JOB_STATUSES.includes(job.status as typeof CANCELABLE_JOB_STATUSES[number]);
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}

export function formatMobileDeepAnalysisProgress(progress: PlaylistDeepAnalysisProgress | null): string {
  if (!progress) return 'Readiness has not been checked yet.';
  if (progress.total === 0) return 'No playlist tracks are available for analysis.';
  const ready = progress.analyzedReal ?? 0;
  if (progress.pending || progress.running) {
    return `${ready}/${progress.total} ready · ${progress.running} running · ${progress.pending} queued`;
  }
  return `${ready}/${progress.total} tracks have real deep analysis`;
}

export default function MobileBoogieMixPanel({
  playlistId,
  playlistName,
  trackCount,
}: {
  playlistId: EntityId;
  playlistName: string;
  trackCount: number;
}) {
  const boogieMix = api.boogiemix;
  const [expanded, setExpanded] = useState(false);
  const [loadingReadiness, setLoadingReadiness] = useState(false);
  const [mixStyle, setMixStyle] = useState<MixStyle>('club_blend');
  const [mixQuality, setMixQuality] = useState<MixQuality>('standard');
  const [mixCrossfade, setMixCrossfade] = useState(16);
  const [deepStatus, setDeepStatus] = useState<BoogieMixDeepAnalysisStatus | null>(null);
  const [deepProgress, setDeepProgress] = useState<PlaylistDeepAnalysisProgress | null>(null);
  const [deepRunning, setDeepRunning] = useState(false);
  const [deepQueuedCount, setDeepQueuedCount] = useState(0);
  const [deepError, setDeepError] = useState('');
  const [mixJobId, setMixJobId] = useState<ClientEntityId | null>(null);
  const [mixJob, setMixJob] = useState<BoogieMixJob | null>(null);
  const [mixOutputs, setMixOutputs] = useState<BoogieMixOutput[]>([]);
  const [mixError, setMixError] = useState('');

  useEffect(() => {
    if (!expanded || !boogieMix) return;
    let cancelled = false;
    setLoadingReadiness(true);
    Promise.allSettled([
      boogieMix.deepAnalysisStatus(),
      boogieMix.playlistDeepAnalysisProgress(playlistId),
      boogieMix.listOutputs(playlistId),
    ]).then(([statusResult, progressResult, outputsResult]) => {
      if (cancelled) return;
      setDeepStatus(statusResult.status === 'fulfilled' ? statusResult.value : null);
      setDeepProgress(progressResult.status === 'fulfilled' ? progressResult.value : null);
      setMixOutputs(outputsResult.status === 'fulfilled' ? outputsResult.value : []);
    }).finally(() => {
      if (!cancelled) setLoadingReadiness(false);
    });
    return () => {
      cancelled = true;
    };
  }, [boogieMix, expanded, playlistId]);

  useEffect(() => {
    if (!mixJobId || !boogieMix || !isActiveJob(mixJob)) return;
    let stopped = false;
    const timer = window.setInterval(async () => {
      try {
        const next = await boogieMix.getJob(mixJobId);
        if (stopped) return;
        setMixJob(next);
        if (!isActiveJob(next)) {
          window.clearInterval(timer);
          const outputs = await boogieMix.listOutputs(playlistId).catch(() => []);
          if (!stopped) setMixOutputs(outputs);
        }
      } catch {
        // Keep the current progress visible and try again on the next poll.
      }
    }, 1200);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [boogieMix, mixJob, mixJobId, playlistId]);

  useEffect(() => {
    if (!deepRunning || !boogieMix) return;
    let stopped = false;
    const timer = window.setInterval(async () => {
      try {
        const progress = await boogieMix.playlistDeepAnalysisProgress(playlistId);
        if (stopped) return;
        setDeepProgress(progress);
        if (progress.pending === 0 && progress.running === 0) {
          window.clearInterval(timer);
          setDeepRunning(false);
        }
      } catch {
        // Retain the latest known progress while the next poll retries.
      }
    }, 2000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [boogieMix, deepRunning, playlistId]);

  const readinessSummary = useMemo(
    () => formatMobileDeepAnalysisProgress(deepProgress),
    [deepProgress],
  );
  const highQualityWarning = mixQuality === 'high_quality' && deepStatus && !deepStatus.runtime?.enabled
    ? `High Quality needs deep analysis. ${deepStatus.runtime?.summary || 'Runtime unavailable.'}`
    : '';
  const latestOutput = mixOutputs[0] ?? null;
  const activeJob = isActiveJob(mixJob);

  const runDeepAnalysis = async () => {
    if (!boogieMix || trackCount === 0 || deepRunning) return;
    setDeepError('');
    setDeepRunning(true);
    try {
      const queued = await boogieMix.queuePlaylistDeepAnalysis(playlistId);
      setDeepQueuedCount(queued.queued);
      const progress = await boogieMix.playlistDeepAnalysisProgress(playlistId);
      setDeepProgress(progress);
      if (progress.pending === 0 && progress.running === 0) setDeepRunning(false);
    } catch (error) {
      setDeepError(errorMessage(error, 'Could not queue deep analysis.'));
      setDeepRunning(false);
    }
  };

  const startMix = async () => {
    if (!boogieMix || trackCount < 2 || activeJob) return;
    setMixError('');
    try {
      const created = await boogieMix.createJob(playlistId, mixStyle, mixQuality, mixCrossfade);
      setMixJobId(created.jobId);
      const job = await boogieMix.getJob(created.jobId);
      setMixJob(job);
      if (!isActiveJob(job)) {
        setMixOutputs(await boogieMix.listOutputs(playlistId).catch(() => []));
      }
    } catch (error) {
      setMixError(errorMessage(error, 'Could not start BoogieMix.'));
    }
  };

  const cancelMix = async () => {
    if (!boogieMix || !mixJobId) return;
    setMixError('');
    try {
      await boogieMix.cancelJob(mixJobId);
      setMixJob(await boogieMix.getJob(mixJobId));
    } catch (error) {
      setMixError(errorMessage(error, 'Could not cancel BoogieMix.'));
    }
  };

  return (
    <section aria-labelledby="mobile-boogiemix-title" style={S.card}>
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls="mobile-boogiemix-body"
        style={S.headerButton}
        onClick={() => setExpanded((current) => !current)}
      >
        <span style={S.mark} aria-hidden="true">✦</span>
        <span style={S.headerCopy}>
          <span id="mobile-boogiemix-title" style={S.title}>Build a BoogieMix</span>
          <span style={S.subtitle}>Analyze transitions and render a continuous playlist mix.</span>
        </span>
        <span style={S.experimental}>Experimental</span>
        <span style={S.chevron} aria-hidden="true">{expanded ? '−' : '+'}</span>
      </button>

      {expanded ? (
        <div id="mobile-boogiemix-body" style={S.body}>
          {!boogieMix ? (
            <div role="status" style={hybridMobileContentStyles.feedback}>
              BoogieMix is unavailable on this server.
            </div>
          ) : (
            <>
              <div style={S.readinessCard}>
                <div style={S.sectionHeader}>
                  <div>
                    <div style={S.sectionTitle}>Deep-analysis readiness</div>
                    <div style={S.meta}>
                      {loadingReadiness ? 'Checking playlist analysis…' : readinessSummary}
                    </div>
                  </div>
                  <span style={S.runtimeBadge}>
                    {deepStatus?.runtime?.enabled ? 'Ready' : deepStatus ? 'Standard only' : 'Checking'}
                  </span>
                </div>
                {deepStatus?.runtime?.summary ? <div style={S.runtimeSummary}>{deepStatus.runtime.summary}</div> : null}
                {deepQueuedCount > 0 ? <div role="status" style={S.queued}>Queued {deepQueuedCount} tracks for analysis.</div> : null}
                {deepError ? <div role="alert" style={S.error}>{deepError}</div> : null}
                <button
                  type="button"
                  style={{
                    ...S.secondaryButton,
                    ...(trackCount === 0 || deepRunning ? hybridMobileContentStyles.disabled : null),
                  }}
                  disabled={trackCount === 0 || deepRunning}
                  onClick={() => void runDeepAnalysis()}
                >
                  {deepRunning ? 'Analysis running…' : 'Analyze playlist'}
                </button>
              </div>

              <div style={S.controlsGrid}>
                <label style={S.fieldLabel}>
                  <span>Mix style</span>
                  <select
                    aria-label="BoogieMix style"
                    value={mixStyle}
                    style={S.select}
                    onChange={(event) => setMixStyle(event.currentTarget.value as MixStyle)}
                  >
                    {STYLE_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>{option.label}</option>
                    ))}
                  </select>
                </label>
                <label style={S.fieldLabel}>
                  <span>Quality</span>
                  <select
                    aria-label="BoogieMix quality"
                    value={mixQuality}
                    style={S.select}
                    onChange={(event) => setMixQuality(event.currentTarget.value as MixQuality)}
                  >
                    <option value="standard">Standard</option>
                    <option value="high_quality">High Quality</option>
                  </select>
                </label>
                <label style={{ ...S.fieldLabel, gridColumn: '1 / -1' }}>
                  <span>Transition length</span>
                  <select
                    aria-label="BoogieMix transition length"
                    value={mixCrossfade}
                    style={S.select}
                    onChange={(event) => setMixCrossfade(Number(event.currentTarget.value))}
                  >
                    {[8, 12, 16, 24, 32, 45].map((seconds) => (
                      <option key={seconds} value={seconds}>{seconds}s blend</option>
                    ))}
                  </select>
                </label>
              </div>

              {highQualityWarning ? <div role="status" style={S.warning}>{highQualityWarning}</div> : null}
              {trackCount < 2 ? <div role="status" style={S.warning}>Add at least two tracks to build a mix.</div> : null}

              <div style={S.actionRow}>
                <button
                  type="button"
                  style={{
                    ...S.primaryButton,
                    ...(trackCount < 2 || activeJob ? hybridMobileContentStyles.disabled : null),
                  }}
                  disabled={trackCount < 2 || activeJob}
                  onClick={() => void startMix()}
                >
                  {activeJob ? 'Building mix…' : `Build ${playlistName}`}
                </button>
                {isCancelableJob(mixJob) ? (
                  <button type="button" style={S.secondaryButton} onClick={() => void cancelMix()}>
                    Cancel
                  </button>
                ) : null}
              </div>

              {mixJob ? (
                <div aria-live="polite" style={S.jobCard}>
                  <div style={S.sectionHeader}>
                    <div style={S.sectionTitle}>Mix {mixJob.status}</div>
                    <strong style={S.progressValue}>{mixJob.progress_percent ?? 0}%</strong>
                  </div>
                  <div style={S.progressTrack}>
                    <div style={{ ...S.progressFill, width: `${Math.max(0, Math.min(100, mixJob.progress_percent ?? 0))}%` }} />
                  </div>
                  <div style={S.meta}>{mixJob.current_step || mixJob.last_message || 'Preparing mix…'}</div>
                  {mixJob.mix_quality ? (
                    <div style={S.meta}>
                      {mixJob.mix_quality === 'high_quality' ? 'High Quality' : 'Standard'}
                      {mixJob.used_deep_analysis ? ' · Deep analysis used' : ' · Standard analysis path'}
                    </div>
                  ) : null}
                  {mixJob.status === 'failed' && mixJob.last_message ? <div role="alert" style={S.error}>{mixJob.last_message}</div> : null}
                </div>
              ) : null}

              {mixError ? <div role="alert" style={S.error}>{mixError}</div> : null}

              {latestOutput ? (
                <div style={S.outputCard}>
                  <div>
                    <div style={S.sectionTitle}>Latest mix</div>
                    <div style={S.meta}>{latestOutput.file_name}</div>
                  </div>
                  <a
                    href={boogieMix.outputDownloadUrl(latestOutput.id)}
                    style={S.downloadButton}
                    download
                  >
                    Download
                  </a>
                </div>
              ) : null}
            </>
          )}
        </div>
      ) : null}
    </section>
  );
}

const S: Record<string, React.CSSProperties> = {
  card: {
    marginBottom: 18,
    overflow: 'hidden',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 18,
    background: 'var(--surface)',
  },
  headerButton: {
    width: '100%',
    minHeight: 72,
    display: 'grid',
    gridTemplateColumns: '36px minmax(0, 1fr) auto 24px',
    alignItems: 'center',
    gap: 10,
    padding: '10px 12px',
    border: 'none',
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    textAlign: 'left',
  },
  mark: {
    width: 36,
    height: 36,
    display: 'grid',
    placeItems: 'center',
    borderRadius: 11,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 17,
    fontWeight: 800,
  },
  headerCopy: { minWidth: 0, display: 'grid', gap: 3 },
  title: { color: 'var(--text)', fontSize: 13, fontWeight: 800 },
  subtitle: { color: 'var(--text-muted)', fontSize: 9, lineHeight: 1.4 },
  experimental: {
    padding: '5px 7px',
    borderRadius: 999,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 8,
    fontWeight: 800,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
  },
  chevron: { color: 'var(--accent)', fontSize: 18, fontWeight: 700, textAlign: 'center' },
  body: {
    display: 'grid',
    gap: 12,
    padding: '14px 12px 12px',
    borderTop: '1px solid var(--divider-subtle)',
  },
  readinessCard: {
    display: 'grid',
    gap: 9,
    padding: 12,
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  sectionHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 10,
  },
  sectionTitle: { color: 'var(--text)', fontSize: 11, fontWeight: 800 },
  meta: { marginTop: 3, color: 'var(--text-muted)', fontSize: 9, lineHeight: 1.45 },
  runtimeBadge: {
    flexShrink: 0,
    padding: '5px 7px',
    borderRadius: 999,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 8,
    fontWeight: 800,
  },
  runtimeSummary: { color: 'var(--text-muted)', fontSize: 9, lineHeight: 1.45 },
  queued: { color: 'var(--success)', fontSize: 9, fontWeight: 700 },
  controlsGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 10,
  },
  fieldLabel: {
    minWidth: 0,
    display: 'grid',
    gap: 5,
    color: 'var(--text-muted)',
    fontSize: 9,
    fontWeight: 700,
  },
  select: {
    ...hybridMobileContentStyles.field,
    minWidth: 0,
    minHeight: 44,
    padding: '0 9px',
    fontSize: 10,
  },
  actionRow: {
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) auto',
    gap: 8,
  },
  primaryButton: {
    minHeight: 48,
    padding: '0 14px',
    border: 'none',
    borderRadius: 12,
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 11,
    fontWeight: 800,
  },
  secondaryButton: {
    minHeight: 44,
    padding: '0 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 10,
    fontWeight: 750,
  },
  warning: {
    padding: 10,
    border: '1px solid color-mix(in srgb, var(--warning) 32%, var(--divider-subtle))',
    borderRadius: 11,
    background: 'color-mix(in srgb, var(--warning) 8%, var(--surface))',
    color: 'var(--warning)',
    fontSize: 9,
    lineHeight: 1.45,
  },
  error: {
    color: 'var(--danger)',
    fontSize: 9,
    lineHeight: 1.45,
  },
  jobCard: {
    display: 'grid',
    gap: 7,
    padding: 12,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  progressValue: { color: 'var(--accent)', fontSize: 11 },
  progressTrack: {
    height: 5,
    overflow: 'hidden',
    borderRadius: 999,
    background: 'var(--surface)',
  },
  progressFill: {
    height: '100%',
    borderRadius: 999,
    background: 'var(--accent)',
  },
  outputCard: {
    minHeight: 60,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    padding: 10,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  downloadButton: {
    minHeight: 44,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '0 12px',
    borderRadius: 11,
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontSize: 10,
    fontWeight: 800,
    textDecoration: 'none',
  },
};
