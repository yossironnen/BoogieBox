/**
 * Defines mobile Mobile Mini Player behavior for the BoogieBox React client.
 */

import React, { useRef } from 'react';
import { api } from '../../api';
import type { PlaybackSnapshot, PlayerState } from '../../components/Player';
import ArtImage from '../../components/ArtImage';

/** Mobile Mini Player is part of this module's public API. */
export default function MobileMiniPlayer({
  snapshot,
  playerState,
  onStateChange,
  onOpenNowPlaying,
  onQuickRate,
}: {
  snapshot: PlaybackSnapshot | null;
  playerState: PlayerState;
  onStateChange: (state: PlayerState) => void;
  onOpenNowPlaying: () => void;
  onQuickRate?: (rating: number | null) => void;
}) {
  const touchStartRef = useRef<{ x: number; y: number } | null>(null);
  const currentTrack = snapshot?.currentTrack ?? playerState.queue[playerState.currentIndex] ?? null;
  if (!currentTrack) return null;

  const progressMax = snapshot?.duration && snapshot.duration > 0 ? snapshot.duration : currentTrack.duration ?? 0;
  const progressPct = progressMax > 0 ? Math.max(0, Math.min(100, ((snapshot?.currentTime ?? 0) / progressMax) * 100)) : 0;
  const canGoNext = playerState.currentIndex < playerState.queue.length - 1;

  return (
    <div
      style={styles.wrap}
      onTouchStart={(event) => {
        const touch = event.touches[0];
        touchStartRef.current = { x: touch.clientX, y: touch.clientY };
      }}
      onTouchEnd={(event) => {
        const start = touchStartRef.current;
        const touch = event.changedTouches[0];
        touchStartRef.current = null;
        if (!start) return;
        if (start.y - touch.clientY > 42 && Math.abs(start.x - touch.clientX) < 80) onOpenNowPlaying();
      }}
    >
      <div style={styles.progressTrack}>
        <div style={{ ...styles.progressFill, width: `${progressPct}%` }} />
      </div>
      <button type="button" style={styles.surface} onClick={onOpenNowPlaying} aria-label={`Open now playing for ${currentTrack.artist || 'Unknown artist'} - ${currentTrack.title || currentTrack.file_name}`}>
        <div style={styles.art}>
          {currentTrack.album_id ? (
            <ArtImage src={api.albumArtUrl(currentTrack.album_id, 300)} alt="" eager={true} imgStyle={styles.artImage} />
          ) : (
            <span style={styles.artFallback} />
          )}
        </div>
        <div style={styles.meta}>
          <div style={styles.artist}>{currentTrack.artist || 'Unknown artist'}</div>
          <div style={styles.title}>{currentTrack.title || currentTrack.file_name}</div>
        </div>
      </button>
      <div style={styles.controls}>
        <button
          type="button"
          style={styles.rate}
          aria-label="Quick rate current track"
          onClick={() => onQuickRate?.((currentTrack.rating ?? 0) >= 4 ? null : 4)}
        >
          ★
        </button>
        <button
          type="button"
          style={styles.play}
          aria-label={playerState.isPlaying ? 'Pause' : 'Play'}
          onClick={() => onStateChange({ ...playerState, isPlaying: !playerState.isPlaying })}
        >
          {playerState.isPlaying ? '❚❚' : '▶'}
        </button>
        <button
          type="button"
          style={styles.skip}
          aria-label="Next track"
          disabled={!canGoNext}
          onClick={() => canGoNext && onStateChange({ ...playerState, currentIndex: playerState.currentIndex + 1, isPlaying: true, playToken: playerState.playToken + 1 })}
        >
          ››
        </button>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  wrap: {
    position: 'fixed',
    left: 0,
    right: 0,
    bottom: 'calc(env(safe-area-inset-bottom, 0px) + 78px)',
    zIndex: 55,
    display: 'grid',
    gridTemplateColumns: '72px minmax(0, 1fr) auto',
    alignItems: 'stretch',
    background: 'linear-gradient(180deg, color-mix(in srgb, var(--surface) 92%, #111) 0%, color-mix(in srgb, var(--surface) 84%, #0c0c0d) 100%)',
    boxShadow: '0 -16px 30px rgba(0,0,0,0.22)',
    borderTop: '1px solid color-mix(in srgb, var(--border) 72%, transparent)',
  },
  progressTrack: { position: 'absolute', left: 0, right: 0, top: 0, height: 2, background: 'rgba(255,255,255,0.18)' },
  progressFill: { height: '100%', background: 'var(--accent)' },
  surface: {
    gridColumn: '1 / 3',
    display: 'grid',
    gridTemplateColumns: '72px minmax(0, 1fr)',
    alignItems: 'center',
    gap: 0,
    border: 'none',
    background: 'transparent',
    color: '#fff',
    padding: 0,
    textAlign: 'left',
    minHeight: 68,
  },
  art: { width: 72, height: 68, overflow: 'hidden', background: 'rgba(255,255,255,0.08)', borderRight: '1px solid color-mix(in srgb, var(--border) 60%, transparent)' },
  artImage: { width: '100%', height: '100%', objectFit: 'cover', display: 'block' },
  artFallback: { display: 'block', width: '100%', height: '100%', background: 'linear-gradient(135deg, rgba(255,255,255,0.14), rgba(255,255,255,0.04))' },
  meta: { minWidth: 0, padding: '10px 12px 10px 14px' },
  artist: { fontSize: 13, color: 'rgba(255,255,255,0.82)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' },
  title: { marginTop: 2, fontSize: 15, fontWeight: 900, color: '#fff', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' },
  controls: { display: 'flex', alignItems: 'center', gap: 6, padding: '0 12px 0 0' },
  rate: { minWidth: 44, minHeight: 44, border: 'none', background: 'transparent', color: 'var(--accent)', fontSize: 20 },
  play: { minWidth: 44, minHeight: 44, border: 'none', background: 'transparent', color: '#fff', fontSize: 22 },
  skip: { minWidth: 44, minHeight: 44, border: 'none', background: 'transparent', color: '#fff', fontSize: 22 },
};
