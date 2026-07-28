/**
 * Defines mobile Mobile Now Playing View behavior for the BoogieBox React client.
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../../api';
import type { PlaybackSnapshot, PlayerEqControls, PlayerState } from '../../components/Player';
import type { AppSettings, AuthUser, ClientEntityId, SonicFingerprint, StemWindow } from '../../types';
import ArtImage from '../../components/ArtImage';
import {
  hybridMobileContentStyles,
  MOBILE_TAB_BAR_DOCK_HEIGHT,
  type HybridThemeMode,
} from '../../hybridPreview';
import { phase2 } from '../../uiPhase2';
import MobileSettingsView from './MobileSettingsView';
import {
  MobileEqualizerSheet,
  MobileVinylSheet,
} from '../components/MobilePlaybackTools';
import MobileConfirmationSheet from '../components/MobileConfirmationSheet';

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
  appSettings,
  onAppSettingsChange,
  hybridThemeMode,
  onHybridThemeModeChange,
  adaptiveAccentEnabled,
  onAdaptiveAccentEnabledChange,
  eqControls,
  playbackMode = 'standard',
  vinylHardcore = false,
  vinylNeedleDrop = false,
  vinylAnalogFxDisabled = false,
  vinylNeedleDropIntensity = 0.65,
  onPlaybackModeChange = () => {},
  onVinylHardcoreChange = () => {},
  onVinylNeedleDropChange = () => {},
  onVinylAnalogFxDisabledChange = () => {},
  onVinylNeedleDropIntensityChange = () => {},
}: {
  currentUser?: AuthUser;
  snapshot: PlaybackSnapshot | null;
  playerState: PlayerState;
  onStateChange: (state: PlayerState) => void;
  appSettings?: AppSettings;
  onAppSettingsChange?: (settings: AppSettings) => void;
  hybridThemeMode?: HybridThemeMode;
  onHybridThemeModeChange?: (mode: HybridThemeMode) => void;
  adaptiveAccentEnabled?: boolean;
  onAdaptiveAccentEnabledChange?: (enabled: boolean) => void;
  eqControls?: PlayerEqControls | null;
  playbackMode?: 'standard' | 'vinyl';
  vinylHardcore?: boolean;
  vinylNeedleDrop?: boolean;
  vinylAnalogFxDisabled?: boolean;
  vinylNeedleDropIntensity?: number;
  onPlaybackModeChange?: (mode: 'standard' | 'vinyl') => void;
  onVinylHardcoreChange?: (enabled: boolean) => void;
  onVinylNeedleDropChange?: (enabled: boolean) => void;
  onVinylAnalogFxDisabledChange?: (enabled: boolean) => void;
  onVinylNeedleDropIntensityChange?: (intensity: number) => void;
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
  const [playbackTool, setPlaybackTool] = useState<'equalizer' | 'vinyl' | null>(null);
  const [clearQueueOpen, setClearQueueOpen] = useState(false);
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
    return (
      <main style={styles.page}>
        <div role="status" style={styles.empty}>
          <div style={styles.emptyMark}>♪</div>
          <div style={styles.emptyTitle}>Nothing playing yet.</div>
          <p style={styles.emptyBody}>Choose something from Home, Browse, Search, or Playlists.</p>
        </div>
      </main>
    );
  }

  return (
    <main style={styles.page}>
      <header style={styles.topBar}>
        <div>
          <div style={styles.eyebrow}>Listening now</div>
          <h1 style={styles.pageTitle}>Now Playing</h1>
        </div>
        <div style={styles.topActions}>
          <button
            type="button"
            style={styles.toolButton}
            onClick={() => setPlaybackTool('equalizer')}
            aria-label="Open equalizer"
          >
            EQ
          </button>
          <button
            type="button"
            style={{
              ...styles.toolButton,
              ...(playbackMode === 'vinyl' ? styles.toolButtonActive : null),
            }}
            onClick={() => setPlaybackTool('vinyl')}
            aria-label="Open Vinyl controls"
          >
            Vinyl
          </button>
          <button type="button" style={styles.settingsButton} onClick={() => setSettingsOpen(true)} aria-label="Open settings">⚙</button>
        </div>
      </header>
      <button
        type="button"
        style={styles.stageButton}
        onClick={advancePanelMode}
        aria-label={panelLabel}
        data-panel-mode={panelMode}
      >
        {panelMode === 'cover' && (
          <div style={styles.coverShell}>
            {track.album_id ? (
              <ArtImage src={api.albumArtUrl(track.album_id, 800)} alt="" eager={true} imgStyle={styles.cover} />
            ) : (
              <div style={styles.coverFallback}>♪</div>
            )}
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
      <div aria-hidden="true" style={styles.stageDots}>
        {(['cover', 'karaoke', 'text', 'fingerprint'] as LyricsPanelMode[]).map((mode) => (
          <span key={mode} style={{ ...styles.stageDot, ...(panelMode === mode ? styles.stageDotActive : null) }} />
        ))}
      </div>
      <section aria-labelledby="mobile-now-playing-track" style={styles.trackIdentity}>
        <h2 id="mobile-now-playing-track" style={styles.title}>{track.title || track.file_name}</h2>
        <div style={styles.sub}>{[track.artist || 'Unknown artist', track.album || 'Unknown album'].join(' • ')}</div>
      </section>
      <div style={styles.progressShell}>
        <div
          role="progressbar"
          aria-label="Playback progress"
          aria-valuemin={0}
          aria-valuemax={Math.max(0, Math.round(max))}
          aria-valuenow={Math.max(0, Math.round(snapshot?.currentTime ?? 0))}
          style={styles.progressTrack}
        >
          <div style={{ ...styles.progressFill, width: `${pct}%` }} />
        </div>
        <div style={styles.progressMeta}>
          <span>{fmt(snapshot?.currentTime ?? 0)}</span>
          <span>{fmt(max)}</span>
        </div>
      </div>
      <div style={styles.controls}>
        <button
          type="button"
          aria-label="Previous track"
          style={{ ...styles.ctrl, ...(!canPrev ? hybridMobileContentStyles.disabled : null) }}
          disabled={!canPrev}
          onClick={() => canPrev && onStateChange({ ...playerState, currentIndex: playerState.currentIndex - 1, isPlaying: true, playToken: playerState.playToken + 1 })}
        >
          ‹‹
        </button>
        <button
          type="button"
          aria-label={playerState.isPlaying ? 'Pause' : 'Play'}
          style={styles.play}
          onClick={() => onStateChange({ ...playerState, isPlaying: !playerState.isPlaying })}
        >
          {playerState.isPlaying ? 'Pause' : 'Play'}
        </button>
        <button
          type="button"
          aria-label="Next track"
          style={{ ...styles.ctrl, ...(!canNext ? hybridMobileContentStyles.disabled : null) }}
          disabled={!canNext}
          onClick={() => canNext && onStateChange({ ...playerState, currentIndex: playerState.currentIndex + 1, isPlaying: true, playToken: playerState.playToken + 1 })}
        >
          ››
        </button>
      </div>
      <section aria-labelledby="mobile-up-next" style={styles.queueSection}>
      <div style={styles.queueTitleBar}>
        <div>
          <h2 id="mobile-up-next" style={styles.queueHeader}>Up Next</h2>
          <div style={styles.queueHint}>Tap to play. Drag to reorder. Swipe left to remove.</div>
        </div>
        <button type="button" style={styles.clearQueue} onClick={() => setClearQueueOpen(true)}>
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
              borderColor: isDropTarget ? 'var(--accent)' : 'var(--divider-subtle)',
              boxShadow: isReordering ? 'var(--shadow-raised)' : 'none',
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
              <span aria-hidden="true" style={styles.queueArtwork}>
                {entry.album_id ? (
                  <ArtImage
                    src={api.albumArtUrl(entry.album_id, 300)}
                    alt=""
                    imgStyle={styles.queueArtworkImage}
                  />
                ) : (
                  <span style={styles.queueArtworkFallback}>♪</span>
                )}
              </span>
              <span style={styles.queueMeta}>
                <span style={styles.queueTitle}>{entry.title || entry.file_name}</span>
                <span style={styles.queueSub}>{entry.artist || 'Unknown artist'}</span>
              </span>
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
        {!playerState.queue.length ? (
          <div role="status" style={styles.queueEmpty}>The queue is empty.</div>
        ) : null}
      </div>
      </section>
      {settingsOpen && currentUser ? (
        <MobileSettingsView
          currentUser={currentUser}
          onClose={() => setSettingsOpen(false)}
          appSettings={appSettings}
          onAppSettingsChange={onAppSettingsChange}
          hybridThemeMode={hybridThemeMode}
          onHybridThemeModeChange={onHybridThemeModeChange}
          adaptiveAccentEnabled={adaptiveAccentEnabled}
          onAdaptiveAccentEnabledChange={onAdaptiveAccentEnabledChange}
        />
      ) : null}
      {playbackTool === 'equalizer' ? (
        <MobileEqualizerSheet
          controls={eqControls ?? null}
          onClose={() => setPlaybackTool(null)}
        />
      ) : null}
      {playbackTool === 'vinyl' ? (
        <MobileVinylSheet
          snapshot={snapshot}
          playbackMode={playbackMode}
          vinylHardcore={vinylHardcore}
          vinylNeedleDrop={vinylNeedleDrop}
          vinylAnalogFxDisabled={vinylAnalogFxDisabled}
          vinylNeedleDropIntensity={vinylNeedleDropIntensity}
          onPlaybackModeChange={onPlaybackModeChange}
          onVinylHardcoreChange={onVinylHardcoreChange}
          onVinylNeedleDropChange={onVinylNeedleDropChange}
          onVinylAnalogFxDisabledChange={onVinylAnalogFxDisabledChange}
          onVinylNeedleDropIntensityChange={onVinylNeedleDropIntensityChange}
          onClose={() => setPlaybackTool(null)}
        />
      ) : null}
      <MobileConfirmationSheet
        open={clearQueueOpen}
        title="Clear the queue?"
        description="This removes every track from the current session and stops playback."
        itemLabel={`${playerState.queue.length} ${playerState.queue.length === 1 ? 'track' : 'tracks'} queued`}
        confirmLabel="Clear queue"
        onClose={() => setClearQueueOpen(false)}
        onConfirm={() => applyQueue([], 0, false)}
      />
    </main>
  );
}

const styles: Record<string, React.CSSProperties> = {
  page: {
    ...phase2.mobilePage,
    paddingBottom: `calc(env(safe-area-inset-bottom, 0px) + ${MOBILE_TAB_BAR_DOCK_HEIGHT + 18}px)`,
  },
  topBar: {
    minHeight: 52,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    marginBottom: 12,
  },
  eyebrow: hybridMobileContentStyles.eyebrow,
  pageTitle: {
    margin: '3px 0 0',
    color: 'var(--text)',
    fontSize: 22,
    fontWeight: 800,
    letterSpacing: -0.5,
    lineHeight: 1.05,
  },
  settingsButton: {
    minWidth: 44,
    minHeight: 44,
    borderRadius: 12,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 17,
  },
  topActions: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  },
  toolButton: {
    minWidth: 44,
    minHeight: 44,
    padding: '0 9px',
    borderRadius: 12,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 10,
    fontWeight: 800,
  },
  toolButtonActive: {
    borderColor: 'var(--accent)',
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
  },
  empty: {
    ...hybridMobileContentStyles.empty,
    marginTop: 20,
  },
  emptyMark: {
    width: 48,
    height: 48,
    display: 'grid',
    placeItems: 'center',
    borderRadius: 12,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 22,
    fontWeight: 800,
  },
  emptyTitle: hybridMobileContentStyles.emptyTitle,
  emptyBody: hybridMobileContentStyles.emptyBody,
  stageButton: {
    width: '100%',
    display: 'block',
    padding: 0,
    overflow: 'hidden',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 18,
    background: 'var(--surface)',
    boxShadow: 'var(--shadow-subtle)',
    color: 'var(--text)',
    textAlign: 'left',
    cursor: 'pointer',
  },
  coverShell: {
    width: '100%',
    position: 'relative',
    aspectRatio: '1 / 1',
    overflow: 'hidden',
    background: 'var(--surface-subtle)',
  },
  cover: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  coverFallback: {
    width: '100%',
    height: '100%',
    display: 'grid',
    placeItems: 'center',
    background: 'linear-gradient(135deg, var(--accent-soft), var(--surface-subtle))',
    color: 'var(--accent)',
    fontSize: 64,
    fontWeight: 800,
  },
  coverHint: {
    position: 'absolute',
    right: 12,
    bottom: 12,
    padding: '7px 10px',
    border: '1px solid color-mix(in srgb, var(--on-accent) 16%, transparent)',
    borderRadius: 999,
    background: 'var(--overlay)',
    color: 'var(--on-accent)',
    fontSize: 10,
    fontWeight: 700,
    backdropFilter: 'blur(10px)',
  },
  lyricsShell: {
    width: '100%',
    aspectRatio: '1 / 1',
    boxSizing: 'border-box',
    padding: '16px 16px 20px',
    overflowY: 'auto',
    background: 'linear-gradient(150deg, var(--surface), var(--surface-subtle))',
  },
  lyricsHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 14,
    color: 'var(--text)',
    fontSize: 11,
    fontWeight: 800,
    letterSpacing: 1.1,
    textTransform: 'uppercase',
  },
  karaokeBadge: {
    padding: '4px 7px',
    borderRadius: 999,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 9,
  },
  lyricsState: { color: 'var(--text-muted)', fontSize: 12, paddingTop: 16 },
  karaokeWrap: { display: 'grid', gap: 10 },
  karaokeLine: { color: 'var(--text-muted)', fontSize: 18, lineHeight: 1.35, fontWeight: 600 },
  karaokeLineActive: {
    color: 'var(--text)',
    textShadow: '0 0 24px color-mix(in srgb, var(--accent) 45%, transparent)',
  },
  lyricsText: { color: 'var(--text)', fontSize: 14, lineHeight: 1.65, whiteSpace: 'pre-wrap' },
  stageDots: {
    minHeight: 24,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 5,
  },
  stageDot: {
    width: 5,
    height: 5,
    display: 'block',
    borderRadius: '50%',
    background: 'var(--border-strong)',
  },
  stageDotActive: {
    width: 16,
    borderRadius: 999,
    background: 'var(--accent)',
  },
  trackIdentity: { minWidth: 0, textAlign: 'center' },
  title: {
    margin: 0,
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 24,
    fontWeight: 800,
    letterSpacing: -0.65,
    lineHeight: 1.1,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  sub: {
    marginTop: 6,
    overflow: 'hidden',
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 600,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  progressShell: { marginTop: 18 },
  progressTrack: {
    height: 5,
    overflow: 'hidden',
    borderRadius: 999,
    background: 'var(--surface-subtle)',
  },
  progressFill: { height: '100%', borderRadius: 999, background: 'var(--accent)' },
  progressMeta: {
    marginTop: 7,
    display: 'flex',
    justifyContent: 'space-between',
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 650,
  },
  controls: {
    display: 'grid',
    gridTemplateColumns: '64px minmax(0, 1fr) 64px',
    gap: 10,
    marginTop: 16,
  },
  ctrl: {
    minHeight: 56,
    borderRadius: 16,
    border: '1px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 21,
  },
  play: {
    minHeight: 56,
    borderRadius: 16,
    border: 'none',
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 800,
  },
  queueSection: { marginTop: 26 },
  queueTitleBar: {
    minHeight: 52,
    marginBottom: 10,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
  },
  queueHeader: {
    margin: 0,
    color: 'var(--text)',
    fontSize: 17,
    fontWeight: 800,
    letterSpacing: -0.25,
  },
  queueHint: {
    marginTop: 4,
    color: 'var(--text-muted)',
    fontSize: 9,
    lineHeight: 1.35,
  },
  clearQueue: {
    minHeight: 44,
    flex: '0 0 auto',
    padding: '0 11px',
    borderRadius: 10,
    border: 'none',
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 10,
    fontWeight: 700,
  },
  queueList: hybridMobileContentStyles.list,
  queueShell: { position: 'relative', overflow: 'hidden', borderRadius: 14 },
  queueDeleteAction: {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'flex-end',
    paddingRight: 16,
    background: 'linear-gradient(90deg, color-mix(in srgb, var(--danger) 8%, var(--surface)), var(--danger))',
    color: 'var(--on-accent)',
    fontSize: 10,
    fontWeight: 800,
  },
  queueRow: {
    width: '100%',
    minHeight: 66,
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) 44px 44px',
    alignItems: 'stretch',
    overflow: 'hidden',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface)',
    color: 'var(--text)',
    textAlign: 'left',
    transition: 'transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease',
    touchAction: 'pan-y',
  },
  queueRowActive: {
    background: 'var(--accent-soft)',
    boxShadow: 'inset 3px 0 0 var(--accent)',
  },
  queueMain: {
    minWidth: 0,
    minHeight: 64,
    display: 'grid',
    gridTemplateColumns: '44px minmax(0, 1fr)',
    alignItems: 'center',
    gap: 9,
    padding: '8px 6px 8px 9px',
    border: 'none',
    background: 'transparent',
    color: 'var(--text)',
    fontFamily: 'inherit',
    textAlign: 'left',
    touchAction: 'pan-y',
  },
  queueArtwork: {
    width: 44,
    height: 44,
    overflow: 'hidden',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
  },
  queueArtworkImage: {
    width: '100%',
    height: '100%',
    display: 'block',
    objectFit: 'cover',
  },
  queueArtworkFallback: {
    width: '100%',
    height: '100%',
    display: 'grid',
    placeItems: 'center',
    background: 'var(--surface-subtle)',
    color: 'var(--accent)',
    fontSize: 16,
    fontWeight: 800,
  },
  queueMeta: { minWidth: 0, display: 'grid', gap: 3 },
  queueHandle: {
    width: 44,
    minHeight: 44,
    border: 'none',
    borderLeft: '1px solid var(--divider-subtle)',
    background: 'transparent',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 20,
    fontWeight: 800,
    touchAction: 'none',
  },
  queueRemove: {
    minWidth: 44,
    minHeight: 44,
    border: 'none',
    borderLeft: '1px solid var(--divider-subtle)',
    background: 'transparent',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 14,
  },
  queueTitle: {
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 12,
    fontWeight: 750,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  queueSub: {
    overflow: 'hidden',
    color: 'var(--text-muted)',
    fontSize: 10,
    fontWeight: 600,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  queueEmpty: hybridMobileContentStyles.feedback,
};

const MOBILE_BINS = 80;
const STEM_CONFIG = [
  { key: 'vocalWindowsJson' as const, label: 'VOCALS', color: 'var(--accent)' },
  { key: 'drumWindowsJson'  as const, label: 'DRUMS',  color: 'var(--warning)' },
  { key: 'bassWindowsJson'  as const, label: 'BASS',   color: 'var(--success)' },
];

export function buildMobileStemBins(windows: StemWindow[], duration: number): number[] {
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
    <div style={{
      width: '100%',
      aspectRatio: '1 / 1',
      boxSizing: 'border-box',
      display: 'flex',
      flexDirection: 'column',
      gap: 12,
      padding: '16px 16px 20px',
      background: 'linear-gradient(150deg, var(--surface), var(--surface-subtle))',
    }}>
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
      background: 'var(--accent-soft)',
      color: 'var(--text)',
      fontWeight: 600,
      whiteSpace: 'nowrap' as const,
    }}>
      {label}
    </span>
  );
}
