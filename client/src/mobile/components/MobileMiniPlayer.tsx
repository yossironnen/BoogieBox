/**
 * Defines mobile Mobile Mini Player behavior for the BoogieBox React client.
 */

import { useRef } from 'react';
import { api } from '../../api';
import type { PlaybackSnapshot, PlayerState } from '../../components/Player';
import ArtImage from '../../components/ArtImage';
import { hybridMobileShellStyles } from '../../hybridPreview';

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
      style={hybridMobileShellStyles.miniPlayer}
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
      <div
        aria-hidden="true"
        style={hybridMobileShellStyles.progressTrack}
      >
        <div style={{ ...hybridMobileShellStyles.progressFill, width: `${progressPct}%` }} />
      </div>
      <button
        type="button"
        style={hybridMobileShellStyles.miniSurface}
        onClick={onOpenNowPlaying}
        aria-label={`Open now playing for ${currentTrack.artist || 'Unknown artist'} - ${currentTrack.title || currentTrack.file_name}`}
      >
        <div style={hybridMobileShellStyles.miniArt}>
          {currentTrack.album_id ? (
            <ArtImage
              src={api.albumArtUrl(currentTrack.album_id, 300)}
              alt=""
              eager={true}
              imgStyle={hybridMobileShellStyles.miniArtImage}
            />
          ) : (
            <span style={hybridMobileShellStyles.miniArtFallback} />
          )}
        </div>
        <div style={hybridMobileShellStyles.miniMeta}>
          <div style={hybridMobileShellStyles.miniTitle}>
            {currentTrack.title || currentTrack.file_name}
          </div>
          <div style={hybridMobileShellStyles.miniArtist}>
            {currentTrack.artist || 'Unknown artist'}
          </div>
        </div>
      </button>
      <div style={hybridMobileShellStyles.miniControls}>
        <button
          type="button"
          aria-pressed={(currentTrack.rating ?? 0) > 0}
          style={{
            ...hybridMobileShellStyles.miniControl,
            ...((currentTrack.rating ?? 0) > 0 ? hybridMobileShellStyles.miniRateActive : {}),
          }}
          aria-label="Quick rate current track"
          onClick={() => onQuickRate?.((currentTrack.rating ?? 0) >= 4 ? null : 4)}
        >
          ★
        </button>
        <button
          type="button"
          style={{
            ...hybridMobileShellStyles.miniControl,
            ...hybridMobileShellStyles.miniPlay,
          }}
          aria-label={playerState.isPlaying ? 'Pause' : 'Play'}
          onClick={() => onStateChange({ ...playerState, isPlaying: !playerState.isPlaying })}
        >
          {playerState.isPlaying ? '❚❚' : '▶'}
        </button>
        <button
          type="button"
          style={{
            ...hybridMobileShellStyles.miniControl,
            ...(!canGoNext ? hybridMobileShellStyles.miniControlDisabled : {}),
          }}
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
