/**
 * Defines the Mix Story detail view — the per-track timeline/breakdown for
 * one rendered BoogieMix output (wip/boogiemix-story-timeline-plan.md).
 * Mirrors PlaylistsView.tsx's playlist drill-down pattern (breadcrumb, 2x2
 * collage header, meta line, action row) so it reads as the same app.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { FastAverageColor } from 'fast-average-color';
import { api } from '../api';
import type { BoogieMixOutput, MixOutputTrackRow, MixTimelineResponse, QueueSource, Track } from '../types';
import { PlaylistArtwork, mixOutputToTrack } from './PlaylistsView';
import { adjustContrast, rgbToHsl, hslToRgb, toHex } from '../hooks/useAdaptiveAccent';
import type { PlaybackSnapshot } from './Player';

// Fixed per-track sizing for the timeline band: a minimum width keeps titles,
// BPM, and waveform legible regardless of track count, at the cost of the
// band growing wider than its panel — the panel scrolls horizontally instead
// of squeezing everything down to fit (wip/boogiemix-story-timeline-plan.md
// follow-up: mockup approved 2026-09-16).
const TRACK_MIN_WIDTH_PX = 130;
const PX_PER_SEC = 0.6;
const RECENTER_THRESHOLD_RATIO = 0.35;

// ─── Icons (icon-first per UI conventions) ────────────────────────────────────

const ChevronLeftIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M15 18l-6-6 6-6"/></svg>;
const PlayIcon = ({ size = 14 }: { size?: number }) => <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>;
const DownloadIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><path d="M7 10l5 5 5-5"/><path d="M12 15V3"/></svg>;
const ClockIcon = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/></svg>;
const TracksIcon = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>;
const CalendarIcon = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/></svg>;
const StoryIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 12h4l3-9 4 18 3-9h4"/></svg>;
const SwapIcon = () => <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 014-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 01-4 4H3"/></svg>;
const RemovedIcon = () => <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>;
const InfoIcon = () => <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>;
const ShareIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><path d="M8.6 13.5l6.8 4M15.4 6.5l-6.8 4"/></svg>;
const RecenterIcon = () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="3"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3"/></svg>;

// ─── Helpers ──────────────────────────────────────────────────────────────────

function formatDuration(sec: number | null | undefined): string {
  if (!sec || sec <= 0) return '–';
  const total = Math.round(sec);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function formatRulerTime(sec: number): string {
  const total = Math.max(0, Math.round(sec));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

/** Segment Layout Item is part of this module's public API. */
export interface SegmentLayoutItem { x: number; width: number; startSec: number; endSec: number; }

/** Lays out each track at a fixed minimum pixel width (scaling up with
 * duration above that floor), so short tracks don't collapse to unreadable
 * slivers when a mix has many tracks. Time<->pixel conversions below walk
 * this per-segment layout rather than assuming a single linear scale across
 * the whole band, since the floor makes the mapping piecewise, not linear. */
export function buildSegmentLayout(tracks: MixOutputTrackRow[]): { items: SegmentLayoutItem[]; totalWidth: number } {
  let x = 0;
  const items = tracks.map(t => {
    const duration = Math.max(0, t.outputEndSec - t.outputStartSec);
    const width = Math.max(TRACK_MIN_WIDTH_PX, duration * PX_PER_SEC);
    const item: SegmentLayoutItem = { x, width, startSec: t.outputStartSec, endSec: t.outputEndSec };
    x += width;
    return item;
  });
  return { items, totalWidth: x };
}

/** Time To X is part of this module's public API. */
export function timeToX(sec: number, layout: SegmentLayoutItem[]): number {
  if (layout.length === 0) return 0;
  if (sec <= layout[0].startSec) return 0;
  const last = layout[layout.length - 1];
  if (sec >= last.endSec) return last.x + last.width;
  for (const item of layout) {
    if (sec >= item.startSec && sec <= item.endSec) {
      const span = item.endSec - item.startSec;
      const ratio = span > 0 ? (sec - item.startSec) / span : 0;
      return item.x + ratio * item.width;
    }
  }
  return 0;
}

/** X To Time is part of this module's public API. */
export function xToTime(x: number, layout: SegmentLayoutItem[]): number {
  if (layout.length === 0) return 0;
  const last = layout[layout.length - 1];
  if (x <= 0) return layout[0].startSec;
  if (x >= last.x + last.width) return last.endSec;
  for (const item of layout) {
    if (x >= item.x && x <= item.x + item.width) {
      const ratio = item.width > 0 ? (x - item.x) / item.width : 0;
      return item.startSec + ratio * (item.endSec - item.startSec);
    }
  }
  return last.endSec;
}

function parseCoverAlbumIds(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed as string[];
  } catch { /* malformed/legacy row */ }
  return [];
}

function parsePeaks(raw: string | null): number[] | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every(v => typeof v === 'number')) return parsed;
  } catch { /* malformed */ }
  return null;
}

function waveformPath(peaks: number[], width: number, height: number): string {
  if (peaks.length === 0) return '';
  const max = Math.max(...peaks, 1);
  const step = width / peaks.length;
  const points = peaks.map((p, i) => {
    const x = i * step;
    const y = height - (Math.max(0, p) / max) * height;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  return `M0,${height} L${points.join(' L')} L${width},${height} Z`;
}

function humanizeTransition(kind: string | null, phraseAligned: boolean, crossfadeSec: number): string {
  const label = kind === 'beatmatch' ? 'Beatmatched'
    : kind === 'energy' ? 'Energy blend'
    : kind ? kind.charAt(0).toUpperCase() + kind.slice(1)
    : 'Blended';
  const parts = [label];
  if (phraseAligned) parts.push('phrase-aligned');
  parts.push(`${Math.round(crossfadeSec)}s blend`);
  return parts.join(' · ');
}

// ─── Per-segment artwork color sampling ────────────────────────────────────────

interface SegmentColor { primary: string; secondary: string; }
const colorCache = new Map<string, SegmentColor>();

/** Samples a dominant color from each track's own artwork, reusing the same
 * `fast-average-color` + contrast-clamp machinery `useAdaptiveAccent.ts`
 * already uses to re-theme the app globally from album/track art — applied
 * here per timeline segment instead (wip/boogiemix-story-timeline-plan.md
 * §4.6). `null` for a track with no resolvable art; the timeline falls back
 * to a neutral fill for that segment, the same way the app-wide hook falls
 * back on a sampling failure. */
function useSegmentColors(tracks: MixOutputTrackRow[]): Map<number, SegmentColor | null> {
  const [colors, setColors] = useState<Map<number, SegmentColor | null>>(new Map());

  useEffect(() => {
    let cancelled = false;
    const fac = new FastAverageColor();
    const pending = tracks
      .map((t, i) => ({ i, albumId: t.albumId }))
      .filter(({ albumId }) => albumId != null);

    pending.forEach(({ i, albumId }) => {
      const url = api.albumArtUrl(albumId as string, 300);
      const cached = colorCache.get(url);
      if (cached) {
        setColors(prev => new Map(prev).set(i, cached));
        return;
      }
      const img = new Image();
      img.crossOrigin = 'anonymous';
      img.src = url;
      img.onload = async () => {
        if (cancelled) return;
        try {
          const result = await fac.getColorAsync(img, { algorithm: 'dominant', mode: 'speed' });
          if (cancelled) return;
          const [r, g, b] = adjustContrast(result.value[0], result.value[1], result.value[2]);
          const primary = toHex(r, g, b);
          const [h, s, l] = rgbToHsl(r, g, b);
          const [sr, sg, sb] = hslToRgb(h, s * 0.6, Math.min(l + 0.18, 0.8));
          const secondary = toHex(sr, sg, sb);
          const value = { primary, secondary };
          colorCache.set(url, value);
          setColors(prev => new Map(prev).set(i, value));
        } catch {
          if (!cancelled) setColors(prev => new Map(prev).set(i, null));
        }
      };
      img.onerror = () => { if (!cancelled) setColors(prev => new Map(prev).set(i, null)); };
    });

    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tracks.map(t => t.albumId ?? '').join(',')]);

  return colors;
}

// ─── Tooltip ──────────────────────────────────────────────────────────────────

interface TooltipState {
  left: number;
  title: string;
  tags: string[];
  detail: string;
}

function TransitionTooltip({ tooltip }: { tooltip: TooltipState }) {
  return (
    <div style={{ ...S.tooltip, left: tooltip.left }}>
      <div style={S.tooltipTitle}>{tooltip.title}</div>
      <div style={S.tooltipRow}>
        {tooltip.tags.map(tag => <span key={tag} style={S.chipMini}>{tag}</span>)}
      </div>
      <div style={S.tooltipRow}>{tooltip.detail}</div>
    </div>
  );
}

// ─── Top-level MixStoryView ────────────────────────────────────────────────────

interface Props {
  output: BoogieMixOutput;
  playTrack: (track: Track, allTracks?: Track[], source?: QueueSource) => void;
  playbackSnapshot?: PlaybackSnapshot | null;
  onBack: () => void;
  onDelete: (output: BoogieMixOutput) => void;
}

/** Mix Story View is part of this module's public API. */
export default function MixStoryView({ output, playTrack, playbackSnapshot, onBack, onDelete }: Props) {
  const [timeline, setTimeline] = useState<MixTimelineResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [hoverTooltip, setHoverTooltip] = useState<TooltipState | null>(null);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [following, setFollowing] = useState(true);
  const [showRecenter, setShowRecenter] = useState(false);
  const bandRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const wasPlayingThisMixRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(false);
    api.boogiemix.timeline(output.id)
      .then(result => { if (!cancelled) setTimeline(result); })
      .catch(() => { if (!cancelled) setError(true); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [output.id]);

  const tracks = useMemo(() => timeline?.tracks ?? [], [timeline]);
  const segmentColors = useSegmentColors(tracks);
  const albumIds = parseCoverAlbumIds(output.cover_album_ids);
  const totalDuration = timeline?.durationSec || output.duration_sec || tracks[tracks.length - 1]?.outputEndSec || 0;

  const segmentLayout = useMemo(() => buildSegmentLayout(tracks), [tracks]);

  const isThisMixPlaying = playbackSnapshot?.currentTrack?.id === `boogiemix:${output.id}`;
  const livePositionSec = isThisMixPlaying ? (playbackSnapshot?.currentTime ?? null) : null;
  const playheadX = livePositionSec != null ? timeToX(livePositionSec, segmentLayout.items) : null;

  // Re-enable follow whenever this mix starts playing fresh (e.g. hitting
  // Play again after it finished, or switching to it from another track).
  useEffect(() => {
    if (isThisMixPlaying && !wasPlayingThisMixRef.current) setFollowing(true);
    wasPlayingThisMixRef.current = isThisMixPlaying;
  }, [isThisMixPlaying]);

  // Keep the playhead centered in the visible viewport while playing and
  // following — a plain scrollLeft assignment (not smooth-scroll) so rapid
  // per-update calls don't stack competing animations or spuriously trip the
  // manual-scroll listener below via a same-frame native "scroll" event.
  useEffect(() => {
    if (!following || playheadX == null) return;
    const scrollEl = scrollRef.current;
    if (!scrollEl) return;
    const viewport = scrollEl.clientWidth;
    const target = Math.max(0, Math.min(segmentLayout.totalWidth - viewport, playheadX - viewport / 2));
    scrollEl.scrollLeft = target;
  }, [following, playheadX, segmentLayout.totalWidth]);

  // A manual wheel/touch/scrollbar-drag gesture breaks the follow lock —
  // listened for on the gesture itself, not the resulting "scroll" event,
  // since our own per-update scrollLeft writes above would otherwise be
  // indistinguishable from a user-driven scroll.
  useEffect(() => {
    const scrollEl = scrollRef.current;
    if (!scrollEl) return;
    const breakFollow = () => setFollowing(false);
    scrollEl.addEventListener('wheel', breakFollow, { passive: true });
    scrollEl.addEventListener('pointerdown', breakFollow, { passive: true });
    scrollEl.addEventListener('touchstart', breakFollow, { passive: true });
    return () => {
      scrollEl.removeEventListener('wheel', breakFollow);
      scrollEl.removeEventListener('pointerdown', breakFollow);
      scrollEl.removeEventListener('touchstart', breakFollow);
    };
    // tracks.length is the proxy for "the scroll container now exists" — the
    // element behind scrollRef only mounts once the timeline finishes
    // loading, so a mount-only ([]) effect here would capture a null ref and
    // never attach these listeners at all.
  }, [tracks.length]);

  // Surface the "recenter" button only once the playhead has actually
  // drifted meaningfully off-screen-center while paused-from-following.
  useEffect(() => {
    const scrollEl = scrollRef.current;
    if (!scrollEl) return;
    const update = () => {
      if (following || !isThisMixPlaying || playheadX == null) { setShowRecenter(false); return; }
      const viewport = scrollEl.clientWidth;
      const viewCenter = scrollEl.scrollLeft + viewport / 2;
      setShowRecenter(Math.abs(playheadX - viewCenter) > viewport * RECENTER_THRESHOLD_RATIO);
    };
    update();
    scrollEl.addEventListener('scroll', update, { passive: true });
    return () => scrollEl.removeEventListener('scroll', update);
  }, [following, isThisMixPlaying, playheadX]);

  const handlePlay = () => {
    const track = mixOutputToTrack(output, output.playlist_name || output.name);
    playTrack(track, [track]);
  };

  const seekTo = (seconds: number) => {
    const track = mixOutputToTrack(output, output.playlist_name || output.name);
    track.startAtSec = Math.max(0, seconds);
    playTrack(track, [track]);
  };

  const handleBandClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!bandRef.current || segmentLayout.items.length === 0) return;
    const rect = bandRef.current.getBoundingClientRect();
    const x = Math.min(rect.width, Math.max(0, e.clientX - rect.left));
    seekTo(xToTime(x, segmentLayout.items));
  };

  const handleRecenter = () => {
    setFollowing(true);
    const scrollEl = scrollRef.current;
    if (scrollEl && playheadX != null) {
      const viewport = scrollEl.clientWidth;
      const target = Math.max(0, Math.min(segmentLayout.totalWidth - viewport, playheadX - viewport / 2));
      if (typeof scrollEl.scrollTo === 'function') {
        scrollEl.scrollTo({ left: target, behavior: 'smooth' });
      } else {
        scrollEl.scrollLeft = target;
      }
    }
  };

  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden', padding: '28px 32px', overflowY: 'auto' }}>
      <button type="button" style={S.backBtn} onClick={onBack} aria-label="Back to Mixes">
        <ChevronLeftIcon /> Mixes
      </button>

      <div style={S.header}>
        <div style={S.collageWrap} aria-label={`${output.name} artwork`}>
          <PlaylistArtwork albumIds={albumIds} responsive />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={S.eyebrow}><StoryIcon /> BOOGIEMIX STORY</div>
          <div style={S.title}>{output.name}</div>
          <div style={S.metaLine}>
            <span><ClockIcon /> {formatDuration(totalDuration)}</span>
            <span><TracksIcon /> {tracks.length} track{tracks.length === 1 ? '' : 's'}</span>
            <span><CalendarIcon /> {new Date(output.created_at.includes('T') ? output.created_at : `${output.created_at.replace(' ', 'T')}Z`).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}</span>
          </div>
        </div>
        <div style={S.actions}>
          <button type="button" style={{ ...S.btn, ...S.btnPrimary }} title="Play" aria-label={`Play ${output.name}`} onClick={handlePlay}>
            <PlayIcon />
          </button>
          <a style={S.btn} title="Download" aria-label={`Download ${output.name}`} href={api.boogiemix.outputDownloadUrl(output.id)}>
            <DownloadIcon />
          </a>
          {timeline?.available && tracks.length > 0 && (
            <a
              style={S.btn}
              title="Share image"
              aria-label={`Download a shareable image for ${output.name}`}
              href={api.boogiemix.storyImageUrl(output.id)}
              download={`${output.name}.png`}
            >
              <ShareIcon />
            </a>
          )}
          <button type="button" style={{ ...S.btn, color: 'var(--danger)' }} title="Delete" aria-label={`Delete ${output.name}`} onClick={() => onDelete(output)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 6h18"/><path d="M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2m3 0-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6"/></svg>
          </button>
        </div>
      </div>

      {loading && <div style={S.empty}>Loading breakdown…</div>}
      {!loading && (error || !timeline) && (
        <div style={S.infoBanner}>
          <InfoIcon /> Couldn&rsquo;t load this mix&rsquo;s breakdown right now.
        </div>
      )}

      {!loading && timeline && !timeline.available && (
        <div style={S.infoBanner}>
          <InfoIcon /> A track-by-track breakdown isn&rsquo;t available for this mix — it was likely created before this view existed, and its build history has since been cleaned up.
        </div>
      )}

      {!loading && timeline && timeline.available && tracks.length > 0 && (
        <>
          <div style={S.sectionTitle}><span style={{ color: 'var(--accent)', display: 'flex' }}><StoryIcon /></span> How this mix was built</div>
          <div style={S.sectionSub}>Every crossfade and section — click anywhere to jump playback.</div>

          <div style={S.timelineWrap}>
            <div style={S.timelineScroll} className="mix-story-timeline-scroll" ref={scrollRef} data-testid="timeline-scroll">
              <div style={{ ...S.timelineBand, width: segmentLayout.totalWidth }} ref={bandRef} onClick={handleBandClick} data-testid="timeline-band">
                {playheadX != null && <div data-testid="playhead" style={{ ...S.playhead, left: playheadX }} />}
                {tracks.map((t, i) => {
                const width = segmentLayout.items[i]?.width ?? TRACK_MIN_WIDTH_PX;
                const color = segmentColors.get(i);
                const peaks = parsePeaks(t.waveformPeaksJson);
                const isActive = activeIndex === i;
                return (
                  <div
                    key={i}
                    data-testid={`timeline-segment-${i}`}
                    style={{
                      ...S.segment,
                      width,
                      background: color
                        ? `linear-gradient(180deg, ${color.primary}a8 0%, ${color.primary}36 65%, transparent 100%)`
                        : 'var(--surface-subtle)',
                      ...(isActive ? S.segmentActive : {}),
                    }}
                    onMouseEnter={() => setActiveIndex(i)}
                    onMouseLeave={() => setActiveIndex(prev => (prev === i ? null : prev))}
                  >
                    {peaks && peaks.length > 0 && (
                      <svg viewBox="0 0 200 92" preserveAspectRatio="none" style={S.segmentSvg}>
                        <path d={waveformPath(peaks, 200, 92)} fill={color ? `${color.secondary}8c` : 'rgba(113,113,122,0.4)'} />
                      </svg>
                    )}
                    <div style={S.segLabel}>
                      <span style={{ ...S.artSwatch, background: color ? color.primary : 'transparent', boxShadow: color ? '0 0 0 1px rgba(255,255,255,.25)' : 'inset 0 0 0 1px rgba(255,255,255,.35)' }} />
                      {t.title}
                    </div>
                    <div style={S.segSub}>
                      {t.artistName}{t.bpm ? ` · ${Math.round(t.bpm)} BPM` : ''}
                    </div>
                    {!t.trackId && (
                      <div style={S.noArtMark}><RemovedIcon /> removed</div>
                    )}
                    {i < tracks.length - 1 && t.crossfadeOutSec > 0 && (
                      <div
                        style={{ ...S.xfade, right: 0 }}
                        onMouseEnter={(e) => {
                          const containerRect = scrollRef.current?.getBoundingClientRect();
                          const xRect = (e.currentTarget as HTMLDivElement).getBoundingClientRect();
                          const left = containerRect ? xRect.left - containerRect.left - 100 : 0;
                          const next = tracks[i + 1];
                          setHoverTooltip({
                            left: Math.max(0, left),
                            title: `${t.title} → ${next.title}`,
                            tags: [
                              t.transitionOutKind === 'beatmatch' ? 'Beatmatched' : (t.transitionOutKind || 'Energy blend'),
                              ...(t.transitionOutPhraseAligned ? ['Phrase-aligned'] : []),
                            ],
                            detail: `${Math.round(t.crossfadeOutSec)}s crossfade`,
                          });
                        }}
                        onMouseLeave={() => setHoverTooltip(null)}
                      >
                        <div style={S.xfadeTag}>⇄ {Math.round(t.crossfadeOutSec)}s</div>
                      </div>
                    )}
                  </div>
                );
                })}
              </div>
              <div style={{ ...S.ruler, width: segmentLayout.totalWidth }}>
                {segmentLayout.items.map((item, i) => (
                  <div key={i} style={{ ...S.tick, width: item.width }}>{formatRulerTime(item.startSec)}</div>
                ))}
              </div>
            </div>
            {hoverTooltip && <TransitionTooltip tooltip={hoverTooltip} />}
            {showRecenter && (
              <button
                type="button"
                data-testid="recenter-btn"
                style={S.recenterBtn}
                title="Recenter"
                aria-label="Recenter timeline on the currently playing position"
                onClick={handleRecenter}
              >
                <RecenterIcon />
              </button>
            )}
            <div style={S.legend}>
              <div style={S.legendItem}><div style={{ ...S.swatch, background: 'linear-gradient(135deg,#4f46a3,#a83e6e,#3f7d4a)' }} /> Segment color — sampled from each track&rsquo;s artwork</div>
              <div style={S.legendItem}><div style={{ ...S.swatch, background: 'rgba(113,113,122,.7)' }} /> No artwork available</div>
            </div>
          </div>

          <div style={{ ...S.sectionTitle, marginTop: 32 }}><TracksIcon /> Track order</div>
          <div style={S.sectionSub}>{tracks.length} tracks · click a row to jump the timeline above.</div>

          <div style={S.trackList}>
            {tracks.map((t, i) => {
              const color = segmentColors.get(i);
              return (
                <React.Fragment key={i}>
                  <div
                    data-testid={`track-row-${i}`}
                    style={{ ...S.trackRow, ...(activeIndex === i ? S.trackRowActive : {}) }}
                    onClick={() => seekTo(t.outputStartSec)}
                    onMouseEnter={() => setActiveIndex(i)}
                    onMouseLeave={() => setActiveIndex(prev => (prev === i ? null : prev))}
                  >
                    <div style={S.idx}>{i + 1}</div>
                    {t.trackId ? (
                      <div style={{ ...S.art, background: color ? color.primary : 'var(--surface-subtle)' }} />
                    ) : (
                      <div style={{ ...S.art, background: 'var(--surface-subtle)', boxShadow: 'inset 0 0 0 1px var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-faint)' }}>
                        <RemovedIcon />
                      </div>
                    )}
                    <div style={S.trackInfo}>
                      <div style={S.trackTitle}>
                        {t.title}
                        {!t.trackId && <span style={S.removedPill}><RemovedIcon /> Removed from library</span>}
                      </div>
                      <div style={S.trackArtist}>{t.artistName || 'Unknown artist'}</div>
                    </div>
                    {t.bpm != null && <div style={S.miniBadge}>{Math.round(t.bpm)} BPM</div>}
                    <div style={S.trackRange}>{formatDuration(t.outputStartSec)} – {formatDuration(t.outputEndSec)}</div>
                  </div>
                  {i < tracks.length - 1 && (
                    <div style={S.transitionRow}>
                      <div style={S.transitionLine} />
                      <div style={S.transitionChip}>
                        <SwapIcon /> {humanizeTransition(t.transitionOutKind, t.transitionOutPhraseAligned, t.crossfadeOutSec)}
                      </div>
                      <div style={S.transitionLine} />
                    </div>
                  )}
                </React.Fragment>
              );
            })}
          </div>

          {timeline.tier === 'reconstructed' && (
            <div style={S.infoBanner}>
              <InfoIcon /> Timeline reconstructed from mix history — waveform detail isn&rsquo;t available for tracks in this mix.
            </div>
          )}
        </>
      )}
    </div>
  );
}

// ─── Styles ───────────────────────────────────────────────────────────────────

const S: Record<string, React.CSSProperties> = {
  backBtn: { display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text-muted)', fontSize: 13, background: 'transparent', border: 'none', cursor: 'pointer', padding: 0, marginBottom: 22, fontFamily: 'inherit' },
  header: { display: 'flex', gap: 22, marginBottom: 26, alignItems: 'center' },
  collageWrap: { width: 108, height: 108, borderRadius: 12, overflow: 'hidden', flexShrink: 0, boxShadow: '0 8px 24px rgba(0,0,0,.4)' },
  eyebrow: { fontSize: 11, letterSpacing: '.08em', textTransform: 'uppercase', color: 'var(--accent)', display: 'flex', alignItems: 'center', gap: 6 },
  title: { fontSize: 26, fontWeight: 600, color: 'var(--text)', margin: '4px 0 6px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  metaLine: { fontSize: 12, color: 'var(--text-muted)', display: 'flex', alignItems: 'center', gap: 14 },
  actions: { display: 'flex', gap: 8, flexShrink: 0 },
  btn: { width: 34, height: 34, borderRadius: 8, border: '1px solid var(--border)', background: 'var(--surface-subtle)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text)', cursor: 'pointer', textDecoration: 'none' },
  btnPrimary: { background: 'var(--accent)', borderColor: 'var(--accent)', color: '#fff' },
  empty: { padding: '48px 0', textAlign: 'center', color: 'var(--text-muted)' },
  infoBanner: { display: 'flex', alignItems: 'center', gap: 10, background: 'var(--surface-subtle)', border: '1px solid var(--border)', borderRadius: 8, padding: '10px 14px', marginTop: 16, fontSize: 12, color: 'var(--text-muted)' },
  sectionTitle: { fontSize: 13, fontWeight: 600, color: 'var(--text)', margin: '18px 0 4px', display: 'flex', alignItems: 'center', gap: 8 },
  sectionSub: { fontSize: 12, color: 'var(--text-muted)', marginBottom: 14 },
  timelineWrap: { background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 12, padding: '18px 18px 14px', position: 'relative' },
  timelineScroll: { overflowX: 'auto', overflowY: 'hidden', borderRadius: 8 },
  timelineBand: { display: 'flex', height: 92, borderRadius: 8, overflow: 'hidden', position: 'relative', cursor: 'pointer' },
  segment: { position: 'relative', display: 'flex', alignItems: 'flex-end', overflow: 'hidden', borderRight: '1px solid rgba(0,0,0,.5)', transition: 'filter .15s ease', flexShrink: 0 },
  segmentActive: { filter: 'brightness(1.2)', boxShadow: 'inset 0 0 0 2px rgba(255,255,255,.35)' },
  playhead: { position: 'absolute', top: 0, bottom: 0, width: 2, background: '#fff', boxShadow: '0 0 8px rgba(255,255,255,.7)', zIndex: 5, pointerEvents: 'none' },
  ruler: { display: 'flex', height: 18, marginTop: 2 },
  tick: { flexShrink: 0, fontSize: 9.5, color: 'var(--text-faint)', borderLeft: '1px solid var(--border)', paddingLeft: 4 },
  recenterBtn: { position: 'absolute', bottom: 34, right: 26, width: 30, height: 30, borderRadius: '50%', border: '1px solid var(--accent)', background: 'var(--accent)', color: '#1a1310', display: 'flex', alignItems: 'center', justifyContent: 'center', cursor: 'pointer', boxShadow: '0 6px 16px rgba(0,0,0,.5)', zIndex: 6 },
  segmentSvg: { position: 'absolute', bottom: 0, left: 0, width: '100%', height: '100%' },
  segLabel: { position: 'absolute', top: 8, left: 9, fontSize: 10.5, color: 'rgba(255,255,255,.92)', fontWeight: 600, textShadow: '0 1px 3px rgba(0,0,0,.8)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: '92%', display: 'flex', alignItems: 'center', gap: 5 },
  artSwatch: { width: 7, height: 7, borderRadius: 2, flexShrink: 0 },
  segSub: { position: 'absolute', top: 24, left: 9, fontSize: 9.5, color: 'rgba(255,255,255,.6)', textShadow: '0 1px 3px rgba(0,0,0,.8)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: '92%' },
  noArtMark: { position: 'absolute', bottom: 8, right: 9, fontSize: 9, color: 'rgba(255,255,255,.45)', display: 'flex', alignItems: 'center', gap: 4 },
  xfade: { position: 'absolute', top: 0, bottom: 0, width: 18, background: 'repeating-linear-gradient(45deg, rgba(99,102,241,.35) 0 4px, rgba(99,102,241,.15) 4px 8px)', zIndex: 3 },
  xfadeTag: { position: 'absolute', bottom: -22, right: -10, fontSize: 9.5, color: 'var(--accent)', background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 5, padding: '2px 5px', whiteSpace: 'nowrap' },
  tooltip: { position: 'absolute', top: -108, width: 220, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 10, padding: '10px 12px', boxShadow: '0 14px 36px rgba(0,0,0,.55)', zIndex: 20, pointerEvents: 'none' },
  tooltipTitle: { fontSize: 12, fontWeight: 600, color: 'var(--text)', marginBottom: 3 },
  tooltipRow: { fontSize: 11, color: 'var(--text-muted)', display: 'flex', alignItems: 'center', gap: 6, marginTop: 5, flexWrap: 'wrap' },
  chipMini: { display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 10, padding: '2px 7px', borderRadius: 10, background: 'color-mix(in srgb, var(--accent) 18%, transparent)', color: 'var(--accent)' },
  legend: { display: 'flex', gap: 14, marginTop: 16, flexWrap: 'wrap' },
  legendItem: { display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--text-muted)' },
  swatch: { width: 10, height: 10, borderRadius: 3 },
  trackList: { marginTop: 8 },
  trackRow: { display: 'flex', alignItems: 'center', gap: 14, padding: '11px 4px', borderBottom: '1px solid var(--border)', cursor: 'pointer', borderRadius: 8 },
  trackRowActive: { background: 'var(--surface-subtle)' },
  idx: { width: 20, textAlign: 'center', fontSize: 12, color: 'var(--text-faint)' },
  art: { width: 38, height: 38, borderRadius: 7, flexShrink: 0 },
  trackInfo: { flex: 1, minWidth: 0 },
  trackTitle: { fontSize: 13.5, color: 'var(--text)', fontWeight: 500, display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  trackArtist: { fontSize: 12, color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  removedPill: { display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 10, color: 'var(--danger)', background: 'color-mix(in srgb, var(--danger) 14%, transparent)', borderRadius: 5, padding: '2px 7px', flexShrink: 0 },
  miniBadge: { fontSize: 10, color: 'var(--text-muted)', background: 'var(--surface-subtle)', border: '1px solid var(--border)', borderRadius: 5, padding: '2px 6px', flexShrink: 0 },
  trackRange: { fontSize: 11.5, color: 'var(--text-muted)', width: 108, textAlign: 'right', flexShrink: 0 },
  transitionRow: { display: 'flex', alignItems: 'center', gap: 8, padding: '6px 4px 6px 52px' },
  transitionLine: { flex: 1, height: 1, background: 'var(--border)' },
  transitionChip: { display: 'flex', alignItems: 'center', gap: 6, fontSize: 10.5, color: 'var(--text-muted)', background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 20, padding: '4px 10px', whiteSpace: 'nowrap' },
};
