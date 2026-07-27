/**
 * Defines the Vinyl Turntable React component and related UI helpers.
 */

import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { hybridAudioPanelStyles } from '../hybridPreview';

interface Props {
  albumArtUrl: string | null;
  title: string;
  isPlaying: boolean;
  currentTime: number;
  duration: number;
  seekDisabled?: boolean;
  onSeek?: (seconds: number, commit: boolean) => void;
  onSeekStart?: () => void;
  onSeekEnd?: (seconds: number) => void;
}

const ARM_PIVOT_X = 132;
const ARM_PIVOT_Y = 12;
const NEEDLE_START_X = 70;
const NEEDLE_START_Y = 4;
const NEEDLE_END_X = 64;
const NEEDLE_END_Y = 41;

function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, v));
}

/** Vinyl Turntable is part of this module's public API. */
export default function VinylTurntable({
  albumArtUrl,
  title,
  isPlaying,
  currentTime,
  duration,
  seekDisabled = false,
  onSeek,
  onSeekStart,
  onSeekEnd,
}: Props) {
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);

  const progress = useMemo(() => {
    if (!Number.isFinite(duration) || duration <= 0) return 0;
    return clamp(currentTime / duration, 0, 1);
  }, [currentTime, duration]);

  const needleX = NEEDLE_START_X + (NEEDLE_END_X - NEEDLE_START_X) * progress;
  const needleY = NEEDLE_START_Y + (NEEDLE_END_Y - NEEDLE_START_Y) * progress;
  const hasSeek = !seekDisabled && !!onSeek && duration > 0;

  const progressFromPointer = useCallback((clientX: number, clientY: number): number | null => {
    if (!hasSeek || !wrapRef.current) return null;
    const rect = wrapRef.current.getBoundingClientRect();
    const px = clientX - rect.left;
    const py = clientY - rect.top;
    const sx = NEEDLE_START_X;
    const sy = NEEDLE_START_Y;
    const ex = NEEDLE_END_X;
    const ey = NEEDLE_END_Y;
    const vx = ex - sx;
    const vy = ey - sy;
    const len2 = vx * vx + vy * vy;
    if (len2 <= 0) return null;
    const t = ((px - sx) * vx + (py - sy) * vy) / len2;
    return clamp(t, 0, 1);
  }, [hasSeek]);

  const seekFromPointer = useCallback((clientX: number, clientY: number, commit: boolean) => {
    if (!onSeek) return;
    const nextProgress = progressFromPointer(clientX, clientY);
    if (nextProgress === null) return;
    onSeek(nextProgress * duration, commit);
  }, [duration, onSeek, progressFromPointer]);

  useEffect(() => {
    const onPointerMove = (e: PointerEvent) => {
      if (!draggingRef.current) return;
      seekFromPointer(e.clientX, e.clientY, false);
    };
    const onPointerUp = (e: PointerEvent) => {
      if (draggingRef.current && onSeekEnd) {
        const nextProgress = progressFromPointer(e.clientX, e.clientY);
        if (nextProgress !== null) onSeekEnd(nextProgress * duration);
      }
      draggingRef.current = false;
    };
    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };
  }, [duration, onSeekEnd, progressFromPointer, seekFromPointer]);

  return (
    <div
      ref={wrapRef}
      data-ui-region="vinyl-turntable"
      role="group"
      aria-label={`${title} vinyl turntable${isPlaying ? ', playing' : ', paused'}`}
      style={V.wrap}
    >
      <div
        style={{
          ...V.disc,
          animationPlayState: isPlaying ? 'running' : 'paused',
          cursor: 'default',
        }}
        title="Vinyl platter"
      >
        <div style={V.grooves} />
        <div style={V.labelOuter}>
          {albumArtUrl ? <img src={albumArtUrl} alt={`${title} vinyl`} style={V.labelArt} /> : <div style={V.labelFallback} />}
          <div style={V.spindle} />
        </div>
      </div>
      <div
        style={{ ...V.tonearm, pointerEvents: hasSeek ? 'auto' : 'none' }}
        onPointerDown={(e) => {
          if (!hasSeek) return;
          onSeekStart?.();
          draggingRef.current = true;
          seekFromPointer(e.clientX, e.clientY, false);
        }}
        title={seekDisabled ? 'Seeking disabled in Hardcore Vinyl mode' : 'Drag needle to seek'}
      >
        <svg width="130" height="142" viewBox="0 -12 130 142" fill="none">
          <line
            x1={ARM_PIVOT_X}
            y1={ARM_PIVOT_Y}
            x2={needleX + 8}
            y2={needleY - 6}
            stroke="var(--text-muted)"
            strokeWidth="4"
            strokeLinecap="round"
          />
          <circle cx={ARM_PIVOT_X} cy={ARM_PIVOT_Y} r="7" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
          <rect x={needleX - 3} y={needleY - 2} width="10" height="6" rx="1.5" fill="var(--accent)" />
          <line x1={needleX + 6} y1={needleY + 2} x2={needleX + 3} y2={needleY + 8} stroke="var(--text)" strokeWidth="2" strokeLinecap="round" />
        </svg>
      </div>
    </div>
  );
}

const V: Record<string, React.CSSProperties> = {
  wrap: {
    ...hybridAudioPanelStyles.vinylDeck,
    position: 'relative',
    width: 130,
    height: 142,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
  },
  disc: {
    width: 118,
    height: 118,
    borderRadius: '50%',
    position: 'relative',
    background: 'radial-gradient(circle at 50% 50%, #303034 0%, #111113 55%, #050506 100%)',
    border: '1px solid var(--border-strong)',
    boxShadow: '0 8px 18px color-mix(in srgb, var(--overlay) 72%, transparent)',
    animation: 'vinyl-spin 2.6s linear infinite',
  },
  grooves: {
    position: 'absolute',
    inset: 8,
    borderRadius: '50%',
    background: 'repeating-radial-gradient(circle at 50% 50%, rgba(255,255,255,0.06) 0px, rgba(255,255,255,0.02) 1px, rgba(0,0,0,0.25) 2px, rgba(0,0,0,0.25) 4px)',
  },
  labelOuter: {
    position: 'absolute',
    left: '50%',
    top: '50%',
    width: 48,
    height: 48,
    transform: 'translate(-50%, -50%)',
    borderRadius: '50%',
    overflow: 'hidden',
    border: '1px solid var(--border)',
    background: 'var(--surface)',
  },
  labelArt: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
    display: 'block',
  },
  labelFallback: {
    width: '100%',
    height: '100%',
    background: 'linear-gradient(135deg, var(--surface), color-mix(in srgb, var(--accent) 25%, var(--surface)))',
  },
  spindle: {
    position: 'absolute',
    left: '50%',
    top: '50%',
    width: 8,
    height: 8,
    transform: 'translate(-50%, -50%)',
    borderRadius: '50%',
    background: 'var(--text-muted)',
    border: '1px solid var(--border)',
  },
  tonearm: {
    position: 'absolute',
    left: 0,
    top: 0,
    touchAction: 'none',
  },
};
