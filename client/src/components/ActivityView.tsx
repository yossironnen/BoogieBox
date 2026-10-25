/**
 * Admin Activity page (desktop): what is playing, downloading and rendering right now, server jobs, and the last
 * 24 hours. Data comes from the shared live feed (`useActivityFeed`, owned by App). Hybrid design: CommandBar
 * header, Home card styles with Vintage overrides, theme tokens only. See wip/activity-dashboard-plan.md.
 */

import React, { useEffect, useMemo, useState } from 'react';
import { api } from '../api';
import { hybridHomeStyles } from '../hybridPreview';
import { useVintageStyle } from '../vintageThemes';
import CommandBar from './CommandBar';
import ArtImage from './ArtImage';
import ConfirmModal from './ConfirmModal';
import { RadioIcon } from './FavouriteIcons';
import { BrowserIcon, PhoneIcon, RendererIcon } from '../remote/OutputIcon';
import {
  ActivityIcon, CancelIcon, ClockIcon, DownloadIcon, MixJobIcon, PauseIcon, PlayIcon, ServerJobsIcon,
  TranscodeIcon, TvIcon, UsersIcon,
} from './activityIcons';
import type { ActivityFeed } from '../activity/useActivityFeed';
import type {
  ActiveMixJob, ActivitySnapshot, ActivitySource, DownloadRow, PlayingRow, RecentRow, ScanRow,
} from '../activity/activityTypes';
import {
  deliveryLabel, downloadDetail, formatRate, formatRelative, mixStepLabel, mixStyleLabel, postScanLabel,
  progressLabel, recentMeta, recentText, sourceLabel,
} from '../activity/format';

export type ActivityFilter = 'all' | 'play' | 'dl' | 'mix' | 'jobs';
export const ACTIVITY_FILTER_STORAGE_KEY = 'boogiebox.activity.filter.v1';
const FILTERS: ActivityFilter[] = ['all', 'play', 'dl', 'mix', 'jobs'];

/** Kind colours: icons and progress fills only. */
const KIND_COLOR = {
  play: 'var(--success)',
  dl: 'var(--warning)',
  mix: 'var(--accent)',
  jobs: 'var(--accent-secondary)',
} as const;

function readFilter(): ActivityFilter {
  try {
    const stored = window.localStorage.getItem(ACTIVITY_FILTER_STORAGE_KEY) as ActivityFilter | null;
    return stored && FILTERS.includes(stored) ? stored : 'all';
  } catch {
    return 'all';
  }
}

function SourceIcon({ source, size = 13 }: { source: ActivitySource; size?: number }) {
  switch (source) {
    case 'subsonic': return <PhoneIcon size={size} />;
    case 'playOn': return <RendererIcon size={size} />;
    case 'dlna': return <TvIcon size={size} />;
    case 'radio': return <RadioIcon size={size} />;
    default: return <BrowserIcon size={size} />;
  }
}

function SourceChip({ source, client }: { source: ActivitySource; client: string | null }) {
  return (
    <span style={S.chip}>
      <SourceIcon source={source} />
      <span style={S.chipText}>{sourceLabel(source, client)}</span>
    </span>
  );
}

function ProgressBar({ fraction, color, label }: { fraction: number; color: string; label: string }) {
  const pct = Math.max(0, Math.min(1, fraction)) * 100;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
      style={S.track}
    >
      <div style={{ ...S.fill, width: `${pct}%`, background: color }} />
    </div>
  );
}

/** Section icon in a round frame; a spinning arc in the section colour runs around it while `busy`. */
function SectionIcon({ icon, color, busy }: { icon: React.ReactNode; color: string; busy: boolean }) {
  return (
    <span
      role={busy ? 'img' : undefined}
      aria-label={busy ? 'Active' : undefined}
      data-busy={busy ? 'true' : 'false'}
      style={{ ...S.iconFrame, color, ...(busy ? {} : S.iconFrameIdle) }}
    >
      {busy && (
        <span
          className="sidebar-scan-spin"
          aria-hidden="true"
          style={{
            ...S.iconRing,
            borderColor: `color-mix(in srgb, ${color} 18%, transparent)`,
            borderTopColor: color,
            borderRightColor: color,
          }}
        />
      )}
      {icon}
    </span>
  );
}

function ActivityCard({ icon, color, title, count, busy = false, children }: {
  icon: React.ReactNode;
  color: string;
  title: string;
  count?: number;
  /** Something is happening in this section: spin the ring around its icon. */
  busy?: boolean;
  children: React.ReactNode;
}) {
  const vintage = useVintageStyle();
  return (
    <section style={{ ...hybridHomeStyles.card, ...(vintage ? vintage.styles.homeCard : {}), ...S.card }}>
      <h2 style={{ ...hybridHomeStyles.cardTitle, ...(vintage ? vintage.styles.homeCardTitle : {}), ...S.cardTitle }}>
        <SectionIcon icon={icon} color={color} busy={busy} />
        <span>{title}</span>
        {count != null && <span style={S.cardCount}>{count}</span>}
      </h2>
      {children}
    </section>
  );
}

function StatTile({ icon, value, label, title }: { icon: React.ReactNode; value: string; label: string; title?: string }) {
  return (
    <div style={S.tile} title={title}>
      <span style={S.tileIcon}>{icon}</span>
      <span style={S.tileText}>
        <span style={S.tileValue}>{value}</span>
        <span style={S.tileLabel}>{label}</span>
      </span>
    </div>
  );
}

/** Position now, moved on from the sample while playing. */
function livePosition(row: PlayingRow, nowMs: number): number | null {
  if (row.positionSec == null) return null;
  if (row.status !== 'playing') return row.positionSec;
  const moved = row.positionSec + Math.max(0, nowMs - row.sampledAtMs) / 1000;
  return row.durationSec ? Math.min(moved, row.durationSec) : moved;
}

function PlayingItem({ row, nowMs }: { row: PlayingRow; nowMs: number }) {
  const position = livePosition(row, nowMs);
  const delivery = deliveryLabel(row.delivery, row.source, row.client);
  const progress = progressLabel(row, position);
  const meta = [row.status === 'paused' ? 'Paused' : null, progress, delivery].filter(Boolean).join(' · ');
  const art = row.albumId ? api.albumArtUrl(row.albumId, 300) : null;
  return (
    <li style={S.row} data-testid="activity-playing-row">
      <span style={S.thumb}>
        {art
          ? <ArtImage src={art} alt="" imgStyle={S.thumbImg} fallback={<SourceIcon source={row.source} size={18} />} />
          : <SourceIcon source={row.source} size={18} />}
      </span>
      <span style={S.main}>
        <span style={S.primary}>{row.title ?? 'Unknown track'}</span>
        {row.artist && <span style={S.secondary}>{row.artist}</span>}
      </span>
      <span style={S.who}>
        {row.user && <span style={S.user}>{row.user}</span>}
        <SourceChip source={row.source} client={row.client} />
      </span>
      <span style={S.progressCell}>
        {position != null && row.durationSec
          ? <ProgressBar fraction={position / row.durationSec} color={KIND_COLOR.play} label={progress ?? ''} />
          : null}
        <span style={S.secondary}>
          {row.status === 'paused' && <PauseIcon size={11} style={S.inlineIcon} />}
          {meta || (row.status === 'streaming' ? delivery ?? '' : '')}
        </span>
      </span>
    </li>
  );
}

function DownloadItem({ row }: { row: DownloadRow }) {
  const fraction = row.totalBytes ? row.bytes / row.totalBytes : null;
  return (
    <li style={S.row} data-testid="activity-download-row">
      <span style={S.main}>
        <span style={S.primary}>{row.label === 'file' ? row.title ?? 'BoogieMix' : 'Offline sync'}</span>
        <span style={S.secondary}>{downloadDetail(row)}</span>
      </span>
      <span style={S.who}>
        {row.user && <span style={S.user}>{row.user}</span>}
        <SourceChip source={row.source} client={row.client} />
      </span>
      <span style={S.progressCell}>
        {fraction != null && <ProgressBar fraction={fraction} color={KIND_COLOR.dl} label={downloadDetail(row)} />}
        {row.rateBps > 0 && <span style={S.secondary}>{formatRate(row.rateBps)}</span>}
      </span>
    </li>
  );
}

function MixJobItem({ job, onCancel }: { job: ActiveMixJob; onCancel: (job: ActiveMixJob) => void }) {
  const step = mixStepLabel(job);
  const from = job.playlistName ? `From ${job.playlistName}` : null;
  const detail = [from, mixStyleLabel(job.style), `${job.trackCount} ${job.trackCount === 1 ? 'track' : 'tracks'}`]
    .filter(Boolean).join(' · ');
  return (
    <li style={S.row} data-testid="activity-mix-row">
      <span style={S.main}>
        <span style={S.primary}>{job.playlistName ?? 'BoogieMix'}</span>
        <span style={S.secondary}>{detail}</span>
      </span>
      <span style={S.who}>{job.user && <span style={S.user}>{job.user}</span>}</span>
      <span style={S.progressCell}>
        <ProgressBar fraction={job.progressPercent / 100} color={KIND_COLOR.mix} label={step} />
        <span style={S.secondary}>{step}</span>
      </span>
      {job.canCancel && (
        <button
          type="button"
          style={S.iconButton}
          title="Cancel mix"
          aria-label="Cancel mix"
          onClick={() => onCancel(job)}
        >
          <CancelIcon size={16} />
        </button>
      )}
    </li>
  );
}

function ScanItem({ scan }: { scan: ScanRow }) {
  const found = scan.filesFound ?? 0;
  const scanned = scan.filesScanned ?? 0;
  const fraction = found > 0 ? scanned / found : 0;
  return (
    <li style={S.row} data-testid="activity-scan-row">
      <span style={S.main}>
        <span style={S.primary}>Library scan · {scan.library ?? 'Library'}</span>
        <span style={S.secondary}>{found > 0 ? `${scanned.toLocaleString()} / ${found.toLocaleString()}` : scan.status}</span>
      </span>
      <span style={S.progressCell}>
        {found > 0 && <ProgressBar fraction={fraction} color={KIND_COLOR.jobs} label={`${Math.round(fraction * 100)}%`} />}
      </span>
    </li>
  );
}

function recentIcon(row: RecentRow) {
  switch (row.kind) {
    case 'play':
    case 'stream':
    case 'playOnStopped':
      return <span style={{ color: KIND_COLOR.play, display: 'inline-flex' }}><PlayIcon size={14} /></span>;
    case 'download':
      return <span style={{ color: KIND_COLOR.dl, display: 'inline-flex' }}><DownloadIcon size={14} /></span>;
    case 'mixFinished':
    case 'mixFailed':
      return <span style={{ color: KIND_COLOR.mix, display: 'inline-flex' }}><MixJobIcon size={14} /></span>;
    default:
      return <span style={{ color: KIND_COLOR.jobs, display: 'inline-flex' }}><ServerJobsIcon size={14} /></span>;
  }
}

function Empty({ text }: { text: string }) {
  return <p style={S.empty}>{text}</p>;
}

interface Props {
  feed: ActivityFeed;
}

export default function ActivityView({ feed }: Props) {
  const { snapshot, connection, updatedAt, refresh } = feed;
  const [filter, setFilter] = useState<ActivityFilter>(readFilter);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [cancelling, setCancelling] = useState<ActiveMixJob | null>(null);
  const [cancelBusy, setCancelBusy] = useState(false);
  const [cancelError, setCancelError] = useState<string | null>(null);

  const anyPlaying = Boolean(snapshot?.playing.some((p) => p.status === 'playing' && p.positionSec != null));
  useEffect(() => {
    if (!anyPlaying) return undefined;
    const timer = setInterval(() => setNowMs(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [anyPlaying]);
  useEffect(() => { setNowMs(Date.now()); }, [snapshot]);

  const chooseFilter = (next: ActivityFilter) => {
    setFilter(next);
    try { window.localStorage.setItem(ACTIVITY_FILTER_STORAGE_KEY, next); } catch { /* per-browser only */ }
  };

  const confirmCancel = async () => {
    if (!cancelling) return;
    setCancelBusy(true);
    setCancelError(null);
    try {
      await api.activity.cancelMixJob(cancelling.id);
      setCancelling(null);
      refresh();
    } catch (error) {
      setCancelError(error instanceof Error ? error.message : String(error));
    } finally {
      setCancelBusy(false);
    }
  };

  const show = (kind: Exclude<ActivityFilter, 'all'>) => filter === 'all' || filter === kind;
  const tabs: Array<{ id: ActivityFilter; label: string; icon: React.ReactNode }> = [
    { id: 'all', label: 'All', icon: <ActivityIcon size={15} /> },
    { id: 'play', label: 'Playback', icon: <PlayIcon size={14} /> },
    { id: 'dl', label: 'Downloads', icon: <DownloadIcon size={15} /> },
    { id: 'mix', label: 'BoogieMix', icon: <MixJobIcon size={15} /> },
    { id: 'jobs', label: 'Server jobs', icon: <ServerJobsIcon size={15} /> },
  ];

  const liveChip = (
    <span style={S.live} data-testid="activity-connection">
      <span
        aria-hidden="true"
        style={{ ...S.liveDot, background: connection === 'live' ? 'var(--success)' : 'var(--warning)' }}
      />
      {connection === 'live' && 'Live'}
      {connection === 'connecting' && 'Reconnecting…'}
      {connection === 'polling' && `Updated ${updatedAt ? formatRelative(updatedAt, nowMs) : '…'}`}
    </span>
  );

  return (
    <div style={S.root}>
      <CommandBar watermark="Activity" style={S.bar} actions={liveChip}>
        <div role="tablist" aria-label="Filter activity" style={S.tabs}>
          {tabs.map((tab) => {
            const active = filter === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                role="tab"
                aria-selected={active}
                style={{ ...S.tab, ...(active ? S.tabActive : {}) }}
                onClick={() => chooseFilter(tab.id)}
              >
                {tab.icon}
                <span>{tab.label}</span>
              </button>
            );
          })}
        </div>
      </CommandBar>
      <div style={S.body}>
        {!snapshot ? (
          <p style={S.empty}>…</p>
        ) : (
          <Dashboard
            snapshot={snapshot}
            nowMs={nowMs}
            show={show}
            onCancel={(job) => { setCancelError(null); setCancelling(job); }}
          />
        )}
      </div>
      {cancelling && (
        <ConfirmModal
          title="Cancel mix"
          message={(
            <>
              {cancelling.user ? `${cancelling.user}'s mix` : 'This mix'}
              {cancelling.playlistName ? ` from ${cancelling.playlistName}` : ''} will be cancelled.
              {cancelError && <span role="alert" style={S.error}>{cancelError}</span>}
            </>
          )}
          confirmLabel="Cancel mix"
          cancelLabel="Keep"
          tone="danger"
          busy={cancelBusy}
          onConfirm={confirmCancel}
          onCancel={() => setCancelling(null)}
        />
      )}
    </div>
  );
}

function Dashboard({ snapshot, nowMs, show, onCancel }: {
  snapshot: ActivitySnapshot;
  nowMs: number;
  show: (kind: 'play' | 'dl' | 'mix' | 'jobs') => boolean;
  onCancel: (job: ActiveMixJob) => void;
}) {
  const { totals, serverJobs } = snapshot;
  const queued = postScanLabel(serverJobs);
  const deepLine = serverJobs.deepAnalysisRunning + serverJobs.deepAnalysisQueued > 0
    ? `Deep analysis · ${serverJobs.deepAnalysisRunning} running, ${serverJobs.deepAnalysisQueued} queued`
    : null;
  const hasJobs = serverJobs.scans.length > 0 || Boolean(queued) || Boolean(deepLine);
  const allQuiet = !snapshot.playing.length && !snapshot.downloads.length && !snapshot.mixJobs.length && !hasJobs;
  const transcodeTitle = totals.subsonicTranscodeLimit != null
    ? `${totals.transcodes} transcodes running · Subsonic limit ${totals.subsonicTranscodeLimit}`
    : `${totals.transcodes} transcodes running`;
  const limitSuffix = totals.subsonicTranscodeLimit != null ? ` / ${totals.subsonicTranscodeLimit}` : '';
  const recentNodes = useMemo(() => snapshot.recent.map((row, i) => (
    <li key={`${row.atMs}-${row.kind}-${i}`} style={S.recentItem}>
      {recentIcon(row)}
      <span style={S.main}>
        <span style={S.recentText}>{recentText(row)}</span>
        <span style={S.secondary}>{recentMeta(row, nowMs)}</span>
      </span>
    </li>
  )), [snapshot.recent, nowMs]);

  return (
    <>
      <div style={S.tiles} aria-live="polite">
        <StatTile icon={<PlayIcon size={28} />} value={String(totals.playing)} label="Playing now" />
        <StatTile icon={<DownloadIcon size={28} />} value={String(totals.downloading)} label="Downloading" />
        <StatTile icon={<MixJobIcon size={28} />} value={String(totals.mixJobs)} label="BoogieMix jobs" />
        <StatTile
          icon={<TranscodeIcon size={28} />}
          value={`${totals.transcodes}${limitSuffix}`}
          label="Transcodes"
          title={transcodeTitle}
        />
        <StatTile icon={<UsersIcon size={28} />} value={String(totals.activeUsers)} label="Active users" />
        <StatTile icon={<ServerJobsIcon size={28} />} value={String(totals.serverJobs)} label="Server jobs" />
      </div>
      <div style={S.columns}>
        <div style={S.mainColumn}>
          {allQuiet && <p style={S.allQuiet}>All quiet.</p>}
          {!allQuiet && show('play') && (
            <ActivityCard icon={<PlayIcon size={14} />} color={KIND_COLOR.play} title="Now playing" count={snapshot.playing.length} busy={snapshot.playing.length > 0}>
              {snapshot.playing.length
                ? <ul style={S.list}>{snapshot.playing.map((row) => <PlayingItem key={row.id} row={row} nowMs={nowMs} />)}</ul>
                : <Empty text="Nothing playing" />}
            </ActivityCard>
          )}
          {!allQuiet && show('dl') && (
            <ActivityCard icon={<DownloadIcon size={14} />} color={KIND_COLOR.dl} title="Downloads" count={snapshot.downloads.length} busy={snapshot.downloads.length > 0}>
              {snapshot.downloads.length
                ? <ul style={S.list}>{snapshot.downloads.map((row) => <DownloadItem key={row.id} row={row} />)}</ul>
                : <Empty text="No downloads" />}
            </ActivityCard>
          )}
          {!allQuiet && show('mix') && (
            <ActivityCard icon={<MixJobIcon size={14} />} color={KIND_COLOR.mix} title="BoogieMix" count={snapshot.mixJobs.length} busy={snapshot.mixJobs.length > 0}>
              {snapshot.mixJobs.length
                ? <ul style={S.list}>{snapshot.mixJobs.map((job) => <MixJobItem key={job.id} job={job} onCancel={onCancel} />)}</ul>
                : <Empty text="No BoogieMix jobs running" />}
            </ActivityCard>
          )}
          {!allQuiet && show('jobs') && (
            <ActivityCard icon={<ServerJobsIcon size={14} />} color={KIND_COLOR.jobs} title="Server jobs" count={totals.serverJobs} busy={hasJobs}>
              {hasJobs ? (
                <>
                  {serverJobs.scans.length > 0 && (
                    <ul style={S.list}>{serverJobs.scans.map((scan) => <ScanItem key={scan.id} scan={scan} />)}</ul>
                  )}
                  {queued && <p style={S.note}>{queued}</p>}
                  {deepLine && <p style={S.note}>{deepLine}</p>}
                </>
              ) : <Empty text="No server jobs running" />}
            </ActivityCard>
          )}
        </div>
        <aside style={S.aside}>
          <ActivityCard icon={<ClockIcon size={14} />} color="var(--accent)" title="Recent">
            {snapshot.recent.length ? <ul style={S.recentList}>{recentNodes}</ul> : <Empty text="Nothing yet" />}
            <p style={S.footer}>Last 24 hours</p>
          </ActivityCard>
        </aside>
      </div>
    </>
  );
}

const S: Record<string, React.CSSProperties> = {
  root: { flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' },
  bar: { padding: '8px 28px' },
  body: { flex: 1, minHeight: 0, overflowY: 'auto', padding: '20px 28px 28px', display: 'flex', flexDirection: 'column', gap: 20 },
  tabs: { display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' },
  tab: {
    display: 'inline-flex', alignItems: 'center', gap: 7, minHeight: 34, padding: '0 13px', borderRadius: 999,
    border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text-muted)', cursor: 'pointer',
    fontFamily: 'inherit', fontSize: 13, fontWeight: 600,
  },
  tabActive: {
    borderColor: 'var(--accent)', color: 'var(--accent)', background: 'color-mix(in srgb, var(--accent) 12%, transparent)',
  },
  live: { display: 'inline-flex', alignItems: 'center', gap: 7, color: 'var(--text-muted)', fontSize: 12, whiteSpace: 'nowrap' },
  liveDot: { width: 8, height: 8, borderRadius: 4, flexShrink: 0 },
  tiles: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))', gap: 12 },
  tile: {
    display: 'flex', alignItems: 'center', gap: 12, padding: '14px 16px', borderRadius: 16,
    border: '1px solid var(--divider-subtle)', background: 'var(--surface)',
  },
  tileIcon: { color: 'var(--accent)', opacity: 0.85, display: 'inline-flex', flexShrink: 0 },
  tileText: { display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 },
  tileValue: { fontSize: 22, fontWeight: 700, fontVariantNumeric: 'tabular-nums', color: 'var(--text)', lineHeight: 1.1 },
  tileLabel: { fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' },
  columns: { display: 'flex', flexWrap: 'wrap', gap: 20, alignItems: 'flex-start' },
  mainColumn: { flex: '999 1 560px', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 20 },
  aside: { flex: '1 1 280px', minWidth: 0 },
  card: { display: 'flex', flexDirection: 'column', gap: 12 },
  cardTitle: { margin: 0, display: 'flex', alignItems: 'center', gap: 10, fontSize: 16 },
  iconFrame: {
    position: 'relative', width: 30, height: 30, flexShrink: 0, borderRadius: '50%', boxSizing: 'border-box',
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  },
  iconFrameIdle: { border: '2px solid var(--divider-subtle)' },
  iconRing: {
    position: 'absolute', inset: 0, borderRadius: '50%', boxSizing: 'border-box', borderWidth: 2, borderStyle: 'solid',
  },
  cardCount: { fontSize: 13, fontWeight: 500, color: 'var(--text-muted)', fontVariantNumeric: 'tabular-nums' },
  list: { listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 8 },
  row: {
    display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '10px 16px', padding: 12, borderRadius: 10,
    background: 'var(--surface-subtle)',
  },
  thumb: {
    width: 44, height: 44, borderRadius: 8, overflow: 'hidden', flexShrink: 0, display: 'flex', alignItems: 'center',
    justifyContent: 'center', background: 'var(--surface-hover)', color: 'var(--text-muted)',
  },
  thumbImg: { width: 44, height: 44, objectFit: 'cover', display: 'block' },
  main: { flex: '2 1 200px', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 3 },
  primary: { fontSize: 14, fontWeight: 600, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  secondary: { fontSize: 12, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  who: { flex: '1 1 150px', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 5, alignItems: 'flex-start' },
  user: { fontSize: 13, fontWeight: 600, color: 'var(--text)' },
  chip: {
    display: 'inline-flex', alignItems: 'center', gap: 5, maxWidth: '100%', fontSize: 12, color: 'var(--text-muted)',
    background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px',
  },
  chipText: { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  progressCell: { flex: '1 1 170px', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 6 },
  track: { height: 5, borderRadius: 3, background: 'var(--divider-subtle)', overflow: 'hidden' },
  fill: { height: 5, borderRadius: 3 },
  inlineIcon: { marginRight: 5, verticalAlign: -1 },
  iconButton: {
    width: 40, height: 40, borderRadius: 10, border: '1px solid var(--border)', background: 'transparent',
    color: 'var(--text-muted)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer',
    flexShrink: 0,
  },
  note: { margin: 0, fontSize: 12, color: 'var(--text-muted)' },
  empty: { margin: 0, fontSize: 13, color: 'var(--text-muted)' },
  allQuiet: { margin: 0, padding: '48px 0', textAlign: 'center', fontSize: 15, color: 'var(--text-muted)' },
  recentList: { listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 12 },
  recentItem: { display: 'flex', gap: 10, alignItems: 'flex-start' },
  recentText: { fontSize: 13, color: 'var(--text)', overflowWrap: 'anywhere' },
  footer: {
    margin: 0, paddingTop: 10, borderTop: '1px solid var(--divider-subtle)', fontSize: 12, color: 'var(--text-muted)',
  },
  error: { display: 'block', marginTop: 8, color: 'var(--danger)' },
};
