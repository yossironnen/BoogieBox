/**
 * Defines the Waveform Bar React component and related UI helpers.
 */

import React, { useMemo, useRef, useState } from 'react';
import type { TrackSection, TransitionWindow } from '../types';

/** Waveform Bar Status is part of this module's public API. */
export type WaveformBarStatus = 'ready' | 'loading' | 'generating' | 'missing' | 'error';

/** Waveform Bar Props is part of this module's public API. */
export interface WaveformBarProps {
  points: number[] | null;
  duration: number;
  currentTime: number;
  status: WaveformBarStatus;
  onSeek: (timeSeconds: number) => void;
  onSeekStart?: () => void;
  onSeekEnd?: (timeSeconds: number) => void;
  sections?: TrackSection[];
  transitionWindows?: TransitionWindow[];
}

const SECTION_COLORS: Record<string, string> = {
  intro:     'color-mix(in srgb, var(--text-muted) 45%, transparent)',
  verse:     'color-mix(in srgb, var(--accent) 70%, transparent)',
  chorus:    '#f5a623cc',
  breakdown: '#7b61ffcc',
  build:     '#e91e63cc',
  drop:      '#ff5722cc',
  outro:     'color-mix(in srgb, var(--text-muted) 45%, transparent)',
};

const DISPLAY_BINS = 180;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/** Format Waveform Time is part of this module's public API. */
export function formatWaveformTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '0:00';
  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
  }
  return `${minutes}:${String(secs).padStart(2, '0')}`;
}

/** Compute Waveform Time From Client X is part of this module's public API. */
export function computeWaveformTimeFromClientX(
  clientX: number,
  rectLeft: number,
  rectWidth: number,
  duration: number,
): number {
  if (!Number.isFinite(duration) || duration <= 0 || rectWidth <= 0) return 0;
  const ratio = clamp((clientX - rectLeft) / rectWidth, 0, 1);
  return ratio * duration;
}

/** Downsample Waveform is part of this module's public API. */
export function downsampleWaveform(points: number[], targetBins = DISPLAY_BINS): number[] {
  if (!points.length || targetBins <= 0) return [];
  if (points.length === targetBins) return points.slice();
  const out = new Array<number>(targetBins).fill(0);
  for (let i = 0; i < targetBins; i += 1) {
    const start = Math.floor((i * points.length) / targetBins);
    const end = Math.max(start + 1, Math.floor(((i + 1) * points.length) / targetBins));
    let max = 0;
    for (let j = start; j < end; j += 1) {
      const value = clamp(Math.round(Number(points[j]) || 0), 0, 255);
      if (value > max) max = value;
    }
    out[i] = max;
  }
  return out;
}

function buildPlaceholderBins(status: WaveformBarStatus, count = DISPLAY_BINS): number[] {
  const base = status === 'error' ? 60 : status === 'missing' ? 50 : 100;
  const wobble = status === 'generating' || status === 'loading' ? 80 : 30;
  return Array.from({ length: count }, (_, idx) => {
    const wave = Math.abs(Math.sin(idx * 0.24)) * wobble;
    return clamp(Math.round(base + wave), 10, 200);
  });
}

/** Waveform Bar is part of this module's public API. */
export default function WaveformBar({
  points,
  duration,
  currentTime,
  status,
  onSeek,
  onSeekStart,
  onSeekEnd,
  sections,
  transitionWindows,
}: WaveformBarProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);
  const [hoverX, setHoverX] = useState<number | null>(null);
  const [hoverTime, setHoverTime] = useState<number | null>(null);

  const bins = useMemo(() => {
    if (points && points.length > 0) return downsampleWaveform(points, DISPLAY_BINS);
    return buildPlaceholderBins(status);
  }, [points, status]);

  const clampedCurrent = clamp(Number(currentTime) || 0, 0, Math.max(duration, 0));
  const playedRatio = duration > 0 ? clamp(clampedCurrent / duration, 0, 1) : 0;
  const playedBins = Math.round(playedRatio * bins.length);
  const playheadLeft = `${(playedRatio * 100).toFixed(3)}%`;
  const showHover = hoverX != null && hoverTime != null;

  const updateHoverFromPointer = (clientX: number): number => {
    const rect = rootRef.current?.getBoundingClientRect();
    if (!rect) return 0;
    const clampedX = clamp(clientX - rect.left, 0, rect.width);
    const time = computeWaveformTimeFromClientX(clientX, rect.left, rect.width, duration);
    setHoverX(clampedX);
    setHoverTime(time);
    return time;
  };

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (duration <= 0) return;
    draggingRef.current = true;
    if (event.currentTarget.setPointerCapture) {
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    onSeekStart?.();
    const next = updateHoverFromPointer(event.clientX);
    onSeek(next);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const next = updateHoverFromPointer(event.clientX);
    if (draggingRef.current && duration > 0) {
      onSeek(next);
    }
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    draggingRef.current = false;
    const next = updateHoverFromPointer(event.clientX);
    onSeek(next);
    onSeekEnd?.(next);
    if (event.currentTarget.releasePointerCapture) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const onPointerLeave = () => {
    if (draggingRef.current) return;
    setHoverX(null);
    setHoverTime(null);
  };

  const hasSections = sections && sections.length > 0 && duration > 0;
  const hasTransitions = transitionWindows && transitionWindows.length > 0 && duration > 0;
  const totalHeight = 32 + (hasSections ? 8 : 0);

  return (
    <div
      ref={rootRef}
      role="slider"
      aria-label="Waveform position"
      aria-valuemin={0}
      aria-valuemax={Math.max(duration, 0)}
      aria-valuenow={clampedCurrent}
      data-testid="waveform-bar"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onPointerLeave={onPointerLeave}
      style={{
        position: 'relative',
        flex: 1,
        height: totalHeight,
        display: 'flex',
        flexDirection: 'column',
        cursor: duration > 0 ? 'pointer' : 'default',
        userSelect: 'none',
        touchAction: 'none',
      }}
    >
      {/* Waveform bars row */}
      <div
        style={{
          position: 'relative',
          width: '100%',
          height: 32,
          display: 'flex',
          alignItems: 'center',
          gap: 1,
          opacity: status === 'error' ? 0.6 : 1,
        }}
      >
        {bins.map((value, idx) => {
          const normalized = clamp(value / 255, 0, 1);
          const barHeight = 4 + normalized * 28;
          const active = idx < playedBins;
          return (
            <div
              key={idx}
              data-testid="waveform-bin"
              data-active={active ? 'true' : 'false'}
              style={{
                flex: 1,
                minWidth: 1,
                height: barHeight,
                borderRadius: 1,
                backgroundColor: active
                  ? 'var(--accent)'
                  : status === 'ready'
                    ? 'color-mix(in srgb, var(--text-muted) 42%, var(--border))'
                    : 'var(--border)',
              }}
            />
          );
        })}

        {/* Transition window markers — rendered behind bars */}
        {hasTransitions && transitionWindows!.map((tw, i) => {
          const left = clamp(tw.start / duration, 0, 1) * 100;
          const width = clamp((tw.end - tw.start) / duration, 0, 1) * 100;
          return (
            <div
              key={i}
              data-testid="transition-window-marker"
              style={{
                position: 'absolute',
                top: 0,
                bottom: 0,
                left: `${left}%`,
                width: `${width}%`,
                backgroundColor: tw.recommended !== false
                  ? 'rgba(76, 175, 80, 0.18)'
                  : 'rgba(255, 193, 7, 0.10)',
                pointerEvents: 'none',
              }}
            />
          );
        })}
      </div>

      {/* Section band strip */}
      {hasSections && (
        <div
          data-testid="section-band"
          style={{
            position: 'relative',
            width: '100%',
            height: 4,
            marginTop: 2,
            borderRadius: 2,
            overflow: 'hidden',
            display: 'flex',
          }}
        >
          {sections!.map((sec, i) => {
            const left = clamp(sec.start / duration, 0, 1) * 100;
            const width = clamp((sec.end - sec.start) / duration, 0, 1) * 100;
            const color = SECTION_COLORS[sec.kind.toLowerCase()] ?? 'color-mix(in srgb, var(--text-muted) 30%, transparent)';
            return (
              <div
                key={i}
                data-testid="section-segment"
                title={sec.kind}
                style={{
                  position: 'absolute',
                  left: `${left}%`,
                  width: `${width}%`,
                  height: '100%',
                  backgroundColor: color,
                }}
              />
            );
          })}
        </div>
      )}

      <div
        data-testid="waveform-playhead"
        style={{
          position: 'absolute',
          top: 2,
          height: 28,
          left: playheadLeft,
          width: 2,
          borderRadius: 2,
          backgroundColor: 'var(--text)',
          boxShadow: '0 0 0 1px rgba(0,0,0,0.18)',
          transform: 'translateX(-1px)',
          pointerEvents: 'none',
        }}
      />

      {showHover && (
        <>
          <div
            data-testid="waveform-hover-marker"
            style={{
              position: 'absolute',
              top: 2,
              bottom: 2,
              left: hoverX!,
              width: 1,
              backgroundColor: 'color-mix(in srgb, var(--text) 65%, transparent)',
              pointerEvents: 'none',
              transform: 'translateX(-0.5px)',
            }}
          />
          <div
            data-testid="waveform-tooltip"
            style={{
              position: 'absolute',
              top: -20,
              left: hoverX!,
              transform: 'translateX(-50%)',
              fontSize: 12,
              lineHeight: '16px',
              padding: '1px 6px',
              borderRadius: 4,
              border: '1px solid var(--border)',
              backgroundColor: 'var(--surface)',
              color: 'var(--text-muted)',
              pointerEvents: 'none',
              whiteSpace: 'nowrap',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            {formatWaveformTime(hoverTime!)}
          </div>
        </>
      )}
    </div>
  );
}
