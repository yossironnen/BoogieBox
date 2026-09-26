/**
 * Rotary (bakelite-style) knob for a 0–1 value, used as the Vintage player
 * volume control. Supports vertical drag, mouse wheel, keyboard and
 * double-click-to-mute, with ARIA slider semantics.
 */

import React, { useEffect, useRef } from 'react';

const SWEEP_DEGREES = 270;
const START_DEGREES = -135;
/** Value change per pixel of vertical drag. */
export const KNOB_DRAG_STEP = 0.005;
/** Value change per arrow key / wheel notch. */
export const KNOB_KEY_STEP = 0.02;
/** Value change per PageUp/PageDown. */
export const KNOB_PAGE_STEP = 0.1;

function clamp01(v: number): number {
  return Math.max(0, Math.min(1, v));
}

/** Maps a 0–1 value to the pointer angle in degrees (−135° … +135°). */
export function knobAngle(value: number): number {
  return START_DEGREES + clamp01(value) * SWEEP_DEGREES;
}

/** Returns the value after a key press, or null when the key isn't handled. */
export function knobValueForKey(key: string, value: number): number | null {
  switch (key) {
    case 'ArrowUp':
    case 'ArrowRight':
      return clamp01(value + KNOB_KEY_STEP);
    case 'ArrowDown':
    case 'ArrowLeft':
      return clamp01(value - KNOB_KEY_STEP);
    case 'PageUp':
      return clamp01(value + KNOB_PAGE_STEP);
    case 'PageDown':
      return clamp01(value - KNOB_PAGE_STEP);
    case 'Home':
      return 0;
    case 'End':
      return 1;
    default:
      return null;
  }
}

/** Rotary Knob is part of this module's public API. */
export default function RotaryKnob({
  value,
  onChange,
  onToggleMute,
  muted = false,
  label = 'Volume',
  caption = 'VOLUME',
  size = 50,
}: {
  value: number;
  onChange: (value: number) => void;
  onToggleMute?: () => void;
  muted?: boolean;
  label?: string;
  caption?: string;
  size?: number;
}) {
  const knobRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startY: number; startValue: number } | null>(null);
  const valueRef = useRef(value);
  valueRef.current = value;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  // Native non-passive wheel listener so the page doesn't scroll while turning the knob.
  useEffect(() => {
    const el = knobRef.current;
    if (!el) return undefined;
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY === 0) return;
      e.preventDefault();
      const step = e.deltaY < 0 ? KNOB_KEY_STEP : -KNOB_KEY_STEP;
      onChangeRef.current(clamp01(valueRef.current + step));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  const pct = Math.round(clamp01(value) * 100);
  const angle = knobAngle(value);

  return (
    <div style={S.wrap}>
      <div
        ref={knobRef}
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-valuetext={muted ? 'Muted' : `${pct}%`}
        title={`${label}: ${muted ? 'muted' : `${pct}%`} — drag, scroll or use arrow keys; double-click to mute`}
        style={{ ...S.knob, width: size, height: size }}
        onPointerDown={(e) => {
          e.preventDefault();
          e.currentTarget.focus();
          e.currentTarget.setPointerCapture?.(e.pointerId);
          dragRef.current = { startY: e.clientY, startValue: value };
        }}
        onPointerMove={(e) => {
          const drag = dragRef.current;
          if (!drag) return;
          onChange(clamp01(drag.startValue + (drag.startY - e.clientY) * KNOB_DRAG_STEP));
        }}
        onPointerUp={(e) => {
          dragRef.current = null;
          e.currentTarget.releasePointerCapture?.(e.pointerId);
        }}
        onPointerCancel={() => { dragRef.current = null; }}
        onKeyDown={(e) => {
          const next = knobValueForKey(e.key, value);
          if (next === null) return;
          e.preventDefault();
          onChange(next);
        }}
        onDoubleClick={() => onToggleMute?.()}
      >
        <div
          aria-hidden="true"
          style={{ ...S.pointerArm, transform: `rotate(${angle}deg)`, opacity: muted ? 0.45 : 1 }}
        >
          <span style={S.pointer} />
        </div>
      </div>
      {caption && <span style={S.caption} aria-hidden="true">{caption}</span>}
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  wrap: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 5,
    flexShrink: 0,
  },
  knob: {
    position: 'relative',
    borderRadius: '50%',
    cursor: 'ns-resize',
    touchAction: 'none',
    background: 'radial-gradient(circle at 35% 30%, #4a3a2e 0, #1c140f 60%, #0a0705 100%)',
    boxShadow: '0 4px 8px rgba(0,0,0,0.6), inset 0 0 0 2px var(--vu-ring, var(--accent))',
    outlineOffset: 3,
  },
  // Full-size rotating layer; the pointer is a short bar at its top edge.
  pointerArm: {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    justifyContent: 'center',
    transition: 'transform 60ms linear',
  },
  pointer: {
    marginTop: '12%',
    width: 2,
    height: '26%',
    borderRadius: 1,
    background: 'var(--vu-ring, var(--accent))',
  },
  caption: {
    fontSize: 10,
    fontWeight: 700,
    letterSpacing: 2,
    color: 'var(--vu-ring, var(--text-muted))',
  },
};
