/**
 * Defines mobile Mobile Now Playing View behavior for the BoogieBox React client.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../../api';
import type { PlaybackSnapshot, PlayerState } from '../../components/Player';
import type { AuthUser, ClientEntityId, SonicFingerprint, StemWindow } from '../../types';
import ArtImage from '../../components/ArtImage';
import { phase2 } from '../../uiPhase2';
import MobileSettingsView from './MobileSettingsView';

function fmt(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '0:00';
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${String(secs).padStart(2, '0')}`;
}

type LyricsPanelMode = 'cover' | 'karaoke' | 'text' | 'fingerprint';
type QueueGesture = {
  pointerId: number;
  source: 'row' | 'handle';
  mode: 'swipe' | 'reorder' | null;
  rowIndex: number;
  key: string;
  startX: number;
  startY: number;
  targetIndex: number;
};

const QUEUE_SWIPE_WIDTH = 84;
const QUEUE_SWIPE_THRESHOLD = 68;
const QUEUE_ROW_HEIGHT = 66;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Mobile Now Playing View is part of this module's public API. */
export default function MobileNowPlayingView({
  currentUser,
  snapshot,
  playerState,
  onStateChange,
}: {
  currentUser?: AuthUser;
  snapshot: PlaybackSnapshot | null;
  playerState: PlayerState;
  onStateChange: (state: PlayerState) => void;
}) {
  const track = snapshot?.currentTrack ?? playerState.queue[playerState.currentIndex] ?? null;
  const [panelMode, setPanelMode] = useState<LyricsPanelMode>('cover');
  const [lyricsLoading, setLyricsLoading] = useState(false);
  const [lyricsError, setLyricsError] = useState<string | null>(null);
  const [lyricsText, setLyricsText] = useState('');
  const [lyricsSynced, setLyricsSynced] = useState<Array<{ time: number; text: string }>>([]);
  const [sonicFingerprint, setSonicFingerprint] = useState<SonicFingerprint | null>(null);
  const [sonicFingerprintLoading, setSonicFingerprintLoading] = useState(false);
  const settledFingerprintTrackId = useRef<ClientEntityId | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [queueGesture, setQueueGesture] = useState<QueueGesture | null>(null);
  const [queueSwipeOffsets, setQueueSwipeOffsets] = useState<Record<string, number>>({});
  const suppressQueueClickRef = useRef(false);

  useEffect(() => {
    setPanelMode('cover');
    setLyricsError(null);
    setLyricsText('');
    setLyricsSynced([]);
    setSonicFingerprint(null);
    setSonicFingerprintLoading(false);
    settledFingerprintTrackId.current = null;
  }, [track?.id]);

  useEffect(() => {
    if (!track?.id || panelMode === 'cover') return;
    let cancelled = false;
    setLyricsLoading(true);
    setLyricsError(null);
    api.trackLyrics(track.id)
      .then((result) => {
        if (cancelled) return;
        setLyricsText(result.lyrics || 'Lyrics not available.');
        setLyricsSynced(Array.isArray(result.synced) ? result.synced : []);
      })
      .catch((error: Error) => {
        if (cancelled) return;
        setLyricsError(error.message || 'Lyrics not found');
      })
      .finally(() => {
        if (!cancelled) setLyricsLoading(false);
      });
    return () => { cancelled = true; };
  }, [panelMode, track?.id]);

  useEffect(() => {
    if (!track?.id || panelMode !== 'fingerprint') return;
    if (settledFingerprintTrackId.current === track.id) return;
    let cancelled = false;
    setSonicFingerprintLoading(true);
    api.trackSonicFingerprint(track.id)
      .then((fp) => {
        if (cancelled) return;
        setSonicFingerprint(fp);
        settledFingerprintTrackId.current = track.id;
      })
      .catch(() => {
        if (cancelled) return;
        settledFingerprintTrackId.current = track.id;
      })
      .finally(() => {
        if (!cancelled) setSonicFingerprintLoading(false);
      });
    return () => { cancelled = true; };
  }, [panelMode, track?.id]);

  const activeSyncedLyricIndex = useMemo(() => {
    if (!lyricsSynced.length) return -1;
    const currentTime = snapshot?.currentTime ?? 0;
    let idx = -1;
    for (let i = 0; i < lyricsSynced.length; i += 1) {
      if (currentTime >= lyricsSynced[i].time) idx = i;
      else break;
    }
    return idx;
  }, [lyricsSynced, snapshot?.currentTime]);

  const canPrev = playerState.currentIndex > 0;
  const canNext = playerState.currentIndex < playerState.queue.length - 1;
  const max = snapshot?.duration && snapshot.duration > 0 ? snapshot.duration : track?.duration ?? 0;
  const pct = max > 0 ? Math.max(0, Math.min(100, ((snapshot?.currentTime ?? 0) / max) * 100)) : 0;
  const advancePanelMode = () => {
    setPanelMode((current) => {
      if (current === 'cover') return 'karaoke';
      if (current === 'karaoke') return 'text';
      if (current === 'text') return 'fingerprint';
      return 'cover';
    });
  };

  const panelLabel = panelMode === 'cover'
    ? 'Show lyrics'
    : panelMode === 'karaoke'
      ? 'Show plain lyrics'
      : panelMode === 'text'
        ? 'Show sonic fingerprint'
        : 'Show album art';

  const queueKey = useCallback((id: ClientEntityId, index: number) => `${id}-${index}`, []);

  const applyQueue = useCallback((queue: PlayerState['queue'], requestedIndex: number, playing = playerState.isPlaying) => {
    onStateChange({
      ...playerState,
      queue,
      currentIndex: clamp(requestedIndex, 0, Math.max(0, queue.length - 1)),
      isPlaying: queue.length > 0 && playing,
    });
  }, [onStateChange, playerState]);

  const removeQueueIndex = useCallback((index: number) => {
    const queue = playerState.queue.filter((_, queueIndex) => queueIndex !== index);
    const nextIndex = index < playerState.currentIndex
      ? playerState.currentIndex - 1
      : Math.min(playerState.currentIndex, Math.max(0, queue.length - 1));
    applyQueue(queue, nextIndex);
  }, [applyQueue, playerState.currentIndex, playerState.queue]);

  const reorderQueue = useCallback((from: number, to: number) => {
    if (from === to || from < 0 || to < 0 || from >= playerState.queue.length || to >= playerState.queue.length) return;
    const queue = [...playerState.queue];
    const [moved] = queue.splice(from, 1);
    queue.splice(to, 0, moved);
    const currentTrack = playerState.queue[playerState.currentIndex];
    const nextIndex = currentTrack ? queue.findIndex((entry) => entry.id === currentTrack.id) : playerState.currentIndex;
    applyQueue(queue, nextIndex >= 0 ? nextIndex : playerState.currentIndex);
  }, [applyQueue, playerState.currentIndex, playerState.queue]);

  const clearQueueGesture = useCallback((key?: string) => {
    if (key) {
      setQueueSwipeOffsets((prev) => {
        if (prev[key] === undefined) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
    }
    setQueueGesture(null);
  }, []);

  const handleQueuePointerDown = useCallback((
    event: React.PointerEvent<HTMLElement>,
    index: number,
    key: string,
    source: 'row' | 'handle',
  ) => {
    if (event.button !== 0) return;
    suppressQueueClickRef.current = false;
    setQueueGesture({
      pointerId: event.pointerId,
      source,
      mode: null,
      rowIndex: index,
      key,
      startX: event.clientX,
      startY: event.clientY,
      targetIndex: index,
    });
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }, []);

  const handleQueuePointerMove = useCallback((event: React.PointerEvent<HTMLElement>) => {
    setQueueGesture((current) => {
      if (!current || current.pointerId !== event.pointerId) return current;
      const deltaX = event.clientX - current.startX;
      const deltaY = event.clientY - current.startY;
      if (current.source === 'handle') {
        const nextMode = current.mode ?? (Math.abs(deltaY) > 6 ? 'reorder' : null);
        if (nextMode !== 'reorder') return current;
        suppressQueueClickRef.current = true;
        return {
          ...current,
          mode: 'reorder',
          targetIndex: clamp(current.rowIndex + Math.round(deltaY / QUEUE_ROW_HEIGHT), 0, playerState.queue.length - 1),
        };
      }
      const nextMode = current.mode ?? (deltaX < -6 && Math.abs(deltaX) > Math.abs(deltaY) ? 'swipe' : null);
      if (nextMode !== 'swipe') return current;
      suppressQueueClickRef.current = true;
      const offset = clamp(deltaX, -QUEUE_SWIPE_WIDTH, 0);
      setQueueSwipeOffsets((prev) => ({ ...prev, [current.key]: offset }));
      return { ...current, mode: 'swipe' };
    });
  }, [playerState.queue.length]);

  const handleQueuePointerEnd = useCallback((event: React.PointerEvent<HTMLElement>) => {
    const current = queueGesture;
    if (!current || current.pointerId !== event.pointerId) return;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    if (current.mode === 'swipe') {
      const offset = queueSwipeOffsets[current.key] ?? 0;
      clearQueueGesture(current.key);
      if (offset <= -QUEUE_SWIPE_THRESHOLD) removeQueueIndex(current.rowIndex);
      return;
    }
    if (current.mode === 'reorder') {
      clearQueueGesture(current.key);
      reorderQueue(current.rowIndex, current.targetIndex);
      return;
    }
    clearQueueGesture(current.key);
  }, [clearQueueGesture, queueGesture, queueSwipeOffsets, removeQueueIndex, reorderQueue]);

  if (!track) {
    return <div style={styles.empty}>Nothing playing yet.</div>;
  }

  return (
    <div style={styles.page}>
      <div style={styles.topBar}>
        <span style={styles.queueHeader}>Now Playing</span>
        <button type="button" style={styles.settingsButton} onClick={() => setSettingsOpen(true)} aria-label="Open settings">⚙</button>
      </div>
      <button type="button" style={styles.coverButton} onClick={advancePanelMode} aria-label={panelLabel}>
        {panelMode === 'cover' && (
          <div style={styles.coverShell}>
            {track.album_id ? <ArtImage src={api.albumArtUrl(track.album_id, 800)} alt="" eager={true} imgStyle={styles.cover} /> : null}
            <div style={styles.coverHint}>Tap for lyrics</div>
          </div>
        )}
        {panelMode === 'fingerprint' && (
          <MobileSonicFingerprintPanel
            fingerprint={sonicFingerprint}
            loading={sonicFingerprintLoading}
            duration={snapshot?.duration ?? track?.duration ?? 0}
            currentTime={snapshot?.currentTime ?? 0}
          />
        )}
        {(panelMode === 'karaoke' || panelMode === 'text') && (
          <div style={styles.lyricsShell}>
            <div style={styles.lyricsHeader}>
              <span>Lyrics</span>
              <span style={styles.karaokeBadge}>{panelMode === 'karaoke' ? 'Karaoke' : 'Text'}</span>
            </div>
            {lyricsLoading && <div style={styles.lyricsState}>Loading lyrics…</div>}
            {!lyricsLoading && lyricsError && <div style={styles.lyricsState}>{lyricsError}</div>}
            {!lyricsLoading && !lyricsError && panelMode === 'karaoke' && lyricsSynced.length > 0 && (
              <div style={styles.karaokeWrap}>
                {lyricsSynced.map((line, index) => (
                  <div key={`${line.time}-${index}`} style={{ ...styles.karaokeLine, ...(index === activeSyncedLyricIndex ? styles.karaokeLineActive : null) }}>
                    {line.text}
                  </div>
                ))}
              </div>
            )}
            {!lyricsLoading && !lyricsError && (panelMode === 'text' || !lyricsSynced.length) && (
              <div style={styles.lyricsText}>{lyricsText || 'Lyrics not available.'}</div>
            )}
          </div>
        )}
      </button>
      <div style={styles.title}>{track.title || track.file_name}</div>
      <div style={styles.sub}>{[track.artist || 'Unknown artist', track.album || 'Unknown album'].join(' • ')}</div>
      <div style={styles.progressShell}>
        <div style={styles.progressTrack}>
          <div style={{ ...styles.progressFill, width: `${pct}%` }} />
        </div>
        <div style={styles.progressMeta}>
          <span>{fmt(snapshot?.currentTime ?? 0)}</span>
          <span>{fmt(max)}</span>
        </div>
      </div>
      <div style={styles.controls}>
        <button type="button" style={styles.ctrl} disabled={!canPrev} onClick={() => canPrev && onStateChange({ ...playerState, currentIndex: playerState.currentIndex - 1, isPlaying: true, playToken: playerState.playToken + 1 })}>‹‹</button>
        <button type="button" style={styles.play} onClick={() => onStateChange({ ...playerState, isPlaying: !playerState.isPlaying })}>
          {playerState.isPlaying ? 'Pause' : 'Play'}
        </button>
        <button type="button" style={styles.ctrl} disabled={!canNext} onClick={() => canNext && onStateChange({ ...playerState, currentIndex: playerState.currentIndex + 1, isPlaying: true, playToken: playerState.playToken + 1 })}>››</button>
      </div>
      <div style={styles.queueTitleBar}>
        <div style={styles.queueHeader}>Up Next</div>
        <button type="button" style={styles.clearQueue} onClick={() => applyQueue([], 0, false)}>
          Clear queue
        </button>
      </div>
      <div style={styles.queueList}>
        {playerState.queue.map((entry, index) => {
          const key = queueKey(entry.id, index);
          const isReordering = queueGesture?.mode === 'reorder' && queueGesture.key === key;
          const isDropTarget = queueGesture?.mode === 'reorder' && queueGesture.targetIndex === index && queueGesture.rowIndex !== index;
          return (
          <div key={key} style={styles.queueShell}>
            <div style={styles.queueDeleteAction}>Remove</div>
            <div
            style={{
              ...styles.queueRow,
              ...(index === playerState.currentIndex ? styles.queueRowActive : null),
              transform: `translateX(${queueSwipeOffsets[key] ?? 0}px)`,
              borderColor: isDropTarget ? 'color-mix(in srgb, var(--accent) 56%, var(--border))' : undefined,
              boxShadow: isReordering ? '0 16px 30px rgba(0,0,0,0.28)' : undefined,
            }}
            onDoubleClick={() => {
              const queue = [...playerState.queue];
              const [picked] = queue.splice(index, 1);
              queue.splice(Math.min(playerState.currentIndex + 1, queue.length), 0, picked);
              applyQueue(queue, playerState.currentIndex);
            }}
          >
            <button
              type="button"
              style={styles.queueMain}
              onPointerDown={(event) => handleQueuePointerDown(event, index, key, 'row')}
              onPointerMove={handleQueuePointerMove}
              onPointerUp={handleQueuePointerEnd}
              onPointerCancel={handleQueuePointerEnd}
              onClick={() => {
                if (suppressQueueClickRef.current) {
                  suppressQueueClickRef.current = false;
                  return;
                }
                onStateChange({ ...playerState, currentIndex: index, isPlaying: true, playToken: playerState.playToken + 1 });
              }}
            >
              <span style={styles.queueTitle}>{entry.title || entry.file_name}</span>
              <span style={styles.queueSub}>{entry.artist || 'Unknown artist'}</span>
            </button>
            <button
              type="button"
              style={styles.queueHandle}
              aria-label={`Reorder ${entry.title || entry.file_name}`}
              onPointerDown={(event) => handleQueuePointerDown(event, index, key, 'handle')}
              onPointerMove={handleQueuePointerMove}
              onPointerUp={handleQueuePointerEnd}
              onPointerCancel={handleQueuePointerEnd}
              onClick={(event) => event.preventDefault()}
            >
              ⋮
            </button>
            <button
              type="button"
              style={styles.queueRemove}
              onClick={() => removeQueueIndex(index)}
              aria-label={`Remove ${entry.title || entry.file_name} from queue`}
            >
              x
            </button>
          </div>
          </div>
        );})}
      </div>
      {settingsOpen && currentUser ? <MobileSettingsView currentUser={currentUser} onClose={() => setSettingsOpen(false)} /> : null}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: phase2.mobilePage,
  topBar: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 },
  settingsButton: { minWidth: 44, minHeight: 44, borderRadius: 14, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 18 },
  empty: { padding: '32px 16px', color: 'var(--text-muted)' },
  coverButton: { width: '100%', border: 'none', background: 'transparent', padding: 0, textAlign: 'left', cursor: 'pointer' },
  coverShell: { ...phase2.mobileHeroCard, width: '100%', aspectRatio: '1 / 1' },
  cover: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  coverHint: { position: 'absolute', right: 14, bottom: 14, padding: '8px 12px', borderRadius: 999, background: 'rgba(0,0,0,0.5)', color: '#fff', fontSize: 12, fontWeight: 700 },
  lyricsShell: { ...phase2.mobileHeroCard, width: '100%', aspectRatio: '1 / 1', padding: '18px 18px 22px', overflowY: 'auto' },
  lyricsHeader: { display: 'flex', justifyContent: 'space-between', alignItems: 'center', color: 'var(--text)', fontSize: 14, fontWeight: 800, letterSpacing: 0.6, textTransform: 'uppercase', marginBottom: 14 },
  karaokeBadge: { color: 'var(--accent)' },
  lyricsState: { color: 'var(--text-muted)', fontSize: 14, paddingTop: 16 },
  karaokeWrap: { display: 'grid', gap: 10 },
  karaokeLine: { color: 'var(--text-muted)', fontSize: 20, lineHeight: 1.35, fontWeight: 600 },
  karaokeLineActive: { color: 'var(--text)', textShadow: '0 0 24px color-mix(in srgb, var(--accent) 45%, transparent)' },
  lyricsText: { color: 'var(--text)', fontSize: 16, lineHeight: 1.6, whiteSpace: 'pre-wrap' },
  title: { ...phase2.mobileTitle, marginTop: 24 },
  sub: { marginTop: 8, color: 'var(--text-muted)', fontSize: 15 },
  progressShell: { marginTop: 24 },
  progressTrack: { height: 6, borderRadius: 999, background: 'rgba(255,255,255,0.08)', overflow: 'hidden' },
  progressFill: { height: '100%', background: 'var(--accent)' },
  progressMeta: { marginTop: 8, display: 'flex', justifyContent: 'space-between', color: 'var(--text-muted)', fontSize: 12 },
  controls: { display: 'grid', gridTemplateColumns: '72px 1fr 72px', gap: 12, marginTop: 24 },
  ctrl: { minHeight: 56, borderRadius: 20, border: '1px solid var(--border)', background: 'rgba(255,255,255,0.03)', color: 'var(--text)', fontSize: 24 },
  play: { minHeight: 56, borderRadius: 20, border: 'none', background: 'var(--accent)', color: '#fff', fontSize: 18, fontWeight: 700 },
  queueTitleBar: { marginTop: 28, marginBottom: 10, display: 'flex', alignItems: 'center', justifyContent: 'space-between' },
  queueHeader: { color: 'var(--text)', fontSize: 14, fontWeight: 700, letterSpacing: 0.8, textTransform: 'uppercase' },
  clearQueue: { minHeight: 38, padding: '0 12px', borderRadius: 999, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text)', fontFamily: 'inherit', fontSize: 12, fontWeight: 700 },
  queueList: { display: 'grid', gap: 8 },
  queueShell: { position: 'relative', overflow: 'hidden', borderRadius: 18 },
  queueDeleteAction: { position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'flex-end', paddingRight: 16, background: 'linear-gradient(90deg, rgba(190,44,37,0.12), rgba(190,44,37,0.9))', color: '#fff', fontSize: 12, fontWeight: 900 },
  queueRow: { ...phase2.mobileMediaRow, width: '100%', textAlign: 'left', padding: '8px 8px 8px 14px', color: 'var(--text)', display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) 36px 44px', alignItems: 'center', gap: 6, transition: 'transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease', touchAction: 'pan-y' },
  queueRowActive: { background: 'color-mix(in srgb, var(--accent) 14%, rgba(255,255,255,0.02))', borderColor: 'color-mix(in srgb, var(--accent) 26%, var(--border))' },
  queueMain: { minWidth: 0, border: 'none', background: 'transparent', color: 'var(--text)', textAlign: 'left', padding: 0, display: 'grid', gap: 4, fontFamily: 'inherit', touchAction: 'pan-y' },
  queueHandle: { width: 36, minHeight: 44, border: 'none', background: 'transparent', color: 'var(--text-muted)', fontFamily: 'inherit', fontSize: 22, fontWeight: 800, touchAction: 'none' },
  queueRemove: { minWidth: 44, minHeight: 44, borderRadius: 12, border: '1px solid var(--border)', background: 'transparent', color: 'var(--text-muted)', fontFamily: 'inherit' },
  queueTitle: { fontSize: 15, fontWeight: 700, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  queueSub: { fontSize: 13, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
};

const MOBILE_BINS = 80;
const STEM_CONFIG = [
  { key: 'vocalWindowsJson' as const, label: 'VOCALS', color: '#e91e63' },
  { key: 'drumWindowsJson'  as const, label: 'DRUMS',  color: '#ff9800' },
  { key: 'bassWindowsJson'  as const, label: 'BASS',   color: '#2196f3' },
];

function buildMobileStemBins(windows: StemWindow[], duration: number): number[] {
  const bins = MOBILE_BINS;
  if (!duration || !windows.length) return new Array<number>(bins).fill(0);
  const out = new Array<number>(bins).fill(0);
  for (let i = 0; i < bins; i++) {
    const s = (i / bins) * duration;
    const e = ((i + 1) / bins) * duration;
    let max = 0;
    for (const w of windows) {
      if (w.end > s && w.start < e) max = Math.max(max, Math.min(1, Math.max(0, w.strength)));
    }
    out[i] = max;
  }
  return out;
}

function MobileSonicFingerprintPanel({
  fingerprint,
  loading,
  duration,
  currentTime,
}: {
  fingerprint: SonicFingerprint | null;
  loading: boolean;
  duration: number;
  currentTime: number;
}) {
  const dur = duration || fingerprint?.sourceDurationSec || 0;
  const playedRatio = dur > 0 ? Math.min(1, Math.max(0, currentTime / dur)) : 0;

  const stemBins = useMemo(() => {
    if (!fingerprint) return null;
    return {
      vocal: buildMobileStemBins(fingerprint.vocalWindowsJson, dur),
      drums: buildMobileStemBins(fingerprint.drumWindowsJson, dur),
      bass:  buildMobileStemBins(fingerprint.bassWindowsJson, dur),
    };
  }, [fingerprint, dur]);

  return (
    <div style={{ ...phase2.mobileHeroCard, width: '100%', aspectRatio: '1 / 1', padding: '18px 18px 22px', boxSizing: 'border-box', display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <span style={{ fontSize: 11, fontWeight: 800, letterSpacing: '0.12em', color: 'var(--accent)', textTransform: 'uppercase' as const }}>
          Sonic Fingerprint ✦
        </span>
        <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>Tap to return</span>
      </div>

      {loading && (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)', fontSize: 14 }}>
          Analyzing…
        </div>
      )}

      {!loading && !fingerprint && (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)', fontSize: 14 }}>
          No analysis available
        </div>
      )}

      {!loading && fingerprint && stemBins && (
        <>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' as const }}>
            {fingerprint.bpmDetected != null && (
              <MobileFpBadge label={`♩ ${Math.round(fingerprint.bpmDetected)} BPM`} />
            )}
            <MobileFpBadge label={`⚡ ${Math.round(fingerprint.energyScoreRefined * 100)}% energy`} />
            <MobileFpBadge label={`◎ ${Math.round(fingerprint.confidence * 100)}% conf`} />
          </div>

          <div style={{ flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'center', gap: 12 }}>
            {STEM_CONFIG.map(({ key, label, color }) => {
              const bins = key === 'vocalWindowsJson' ? stemBins.vocal
                         : key === 'drumWindowsJson'  ? stemBins.drums
                         : stemBins.bass;
              const playedCount = Math.round(playedRatio * bins.length);
              return (
                <div key={key} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', color, width: 46, flexShrink: 0, textAlign: 'right' as const }}>{label}</span>
                  <div style={{ flex: 1, height: 18, display: 'flex', alignItems: 'center', gap: 0.5 }}>
                    {bins.map((strength, i) => (
                      <div
                        key={i}
                        style={{
                          flex: 1,
                          minWidth: 1,
                          height: `${Math.max(2, strength * 18)}px`,
                          borderRadius: 1,
                          backgroundColor: color,
                          opacity: i < playedCount
                            ? Math.min(1, Math.max(0.35, 0.4 + strength * 0.6))
                            : Math.min(0.7, Math.max(0.12, 0.18 + strength * 0.55)),
                        }}
                      />
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          {fingerprint.demucsModel && (
            <div style={{ fontSize: 10, color: 'var(--text-muted)', textAlign: 'center' as const }}>{fingerprint.demucsModel}</div>
          )}
        </>
      )}
    </div>
  );
}

function MobileFpBadge({ label }: { label: string }) {
  return (
    <span style={{
      fontSize: 11,
      padding: '3px 10px',
      borderRadius: 6,
      border: '1px solid color-mix(in srgb, var(--accent) 35%, var(--border))',
      background: 'color-mix(in srgb, var(--accent) 10%, var(--bg))',
      color: 'var(--text)',
      fontWeight: 600,
      whiteSpace: 'nowrap' as const,
    }}>
      {label}
    </span>
  );
}
