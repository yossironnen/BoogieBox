/**
 * Defines the Star Rating React component and related UI helpers.
 */

import React, { useMemo, useState } from 'react';

type StarRatingProps = {
  value: number | null | undefined;
  onChange?: (value: number | null) => void | Promise<void>;
  ariaLabel: string;
  size?: 'compact' | 'regular' | 'hero';
  subdued?: boolean;
  showValue?: boolean;
};

const SIZE_PRESETS = {
  compact: { star: 16, gap: 3, valueFont: 13 },
  regular: { star: 18, gap: 4, valueFont: 14 },
  hero: { star: 20, gap: 5, valueFont: 15 },
} as const;

const STAR = '\u2605';

function clampStep(value: number): number {
  const rounded = Math.round(value * 2) / 2;
  return Math.max(0.5, Math.min(5, rounded));
}

function StarGlyph({
  size,
  fill,
  activeColor,
  baseColor,
  rated,
}: {
  size: number;
  fill: number;
  activeColor: string;
  baseColor: string;
  rated: boolean;
}) {
  return (
    <span
      aria-hidden="true"
      style={{
        position: 'relative',
        display: 'inline-flex',
        width: size,
        height: size,
        lineHeight: 1,
        fontSize: size,
        color: baseColor,
        flexShrink: 0,
        opacity: rated ? 1 : 0.72,
      }}
    >
      <span
        style={{
          position: 'absolute',
          inset: 0,
          textShadow: rated ? '0 0 0.35px currentColor' : 'none',
        }}
      >
        {STAR}
      </span>
      <span
        style={{
          position: 'absolute',
          inset: 0,
          width: `${Math.max(0, Math.min(1, fill)) * 100}%`,
          overflow: 'hidden',
          color: activeColor,
          whiteSpace: 'nowrap',
          textShadow: '0 0 0.45px currentColor',
        }}
      >
        {STAR}
      </span>
    </span>
  );
}

/** Star Rating is part of this module's public API. */
export default function StarRating({
  value,
  onChange,
  ariaLabel,
  size = 'regular',
  subdued = false,
  showValue = false,
}: StarRatingProps) {
  const [hoverValue, setHoverValue] = useState<number | null>(null);
  const [focusVisible, setFocusVisible] = useState(false);
  const preset = SIZE_PRESETS[size];
  const editable = typeof onChange === 'function';
  const displayValue = hoverValue ?? value ?? 0;
  const rated = (value ?? 0) > 0;
  const activeColor = subdued
    ? 'color-mix(in srgb, #f6c453 82%, #fff 18%)'
    : 'color-mix(in srgb, #f4b93a 88%, #fff 12%)';
  const baseColor = subdued
    ? 'color-mix(in srgb, var(--text-muted) 24%, transparent)'
    : 'color-mix(in srgb, var(--text-muted) 34%, transparent)';
  const segments = useMemo(() => Array.from({ length: 10 }, (_, idx) => (idx + 1) / 2), []);

  const commitValue = useMemo(
    () => async (nextValue: number) => {
      if (!onChange) return;
      const normalized = clampStep(nextValue);
      await onChange(value === normalized ? null : normalized);
    },
    [onChange, value],
  );

  const currentValueText = value ? `${value.toFixed(1).replace(/\.0$/, '')} / 5` : 'Unrated';

  return (
    <div
      role={editable ? 'slider' : 'img'}
      aria-label={ariaLabel}
      aria-valuemin={editable ? 0 : undefined}
      aria-valuemax={editable ? 5 : undefined}
      aria-valuenow={editable ? (value ?? 0) : undefined}
      aria-valuetext={currentValueText}
      tabIndex={editable ? 0 : undefined}
      title={editable ? `${ariaLabel}: ${currentValueText}` : currentValueText}
      onMouseLeave={() => setHoverValue(null)}
      onFocus={() => setFocusVisible(true)}
      onBlur={() => {
        setFocusVisible(false);
        setHoverValue(null);
      }}
      onKeyDown={(event) => {
        if (!editable || !onChange) return;
        if (event.key === 'ArrowRight' || event.key === 'ArrowUp') {
          event.preventDefault();
          void onChange(value == null ? 0.5 : Math.min(5, value + 0.5));
        } else if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') {
          event.preventDefault();
          if (value == null || value <= 0.5) void onChange(null);
          else void onChange(Math.max(0.5, value - 0.5));
        } else if (event.key === 'Home') {
          event.preventDefault();
          void onChange(0.5);
        } else if (event.key === 'End') {
          event.preventDefault();
          void onChange(5);
        } else if (event.key === 'Delete' || event.key === 'Backspace' || event.key === '0') {
          event.preventDefault();
          void onChange(null);
        }
      }}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        borderRadius: 6,
        padding: editable || rated ? '2px 4px' : 0,
        background: rated ? 'color-mix(in srgb, #f4b93a 10%, transparent)' : 'transparent',
        outline: focusVisible ? '2px solid color-mix(in srgb, var(--accent) 44%, transparent)' : 'none',
        outlineOffset: 1,
      }}
    >
      <div style={{ position: 'relative', display: 'inline-flex', gap: preset.gap }}>
        {Array.from({ length: 5 }, (_, idx) => {
          const starFill = Math.max(0, Math.min(1, displayValue - idx));
          return (
            <StarGlyph
              key={idx}
              size={preset.star}
              fill={starFill}
              activeColor={activeColor}
              baseColor={baseColor}
              rated={rated || starFill > 0}
            />
          );
        })}
        {editable && (
          <div style={{ position: 'absolute', inset: 0, display: 'grid', gridTemplateColumns: 'repeat(10, 1fr)' }}>
            {segments.map((segmentValue) => (
              <button
                key={segmentValue}
                type="button"
                aria-label={`Set rating to ${segmentValue} stars`}
                tabIndex={-1}
                onMouseEnter={() => setHoverValue(segmentValue)}
                onClick={(event) => {
                  event.stopPropagation();
                  void commitValue(segmentValue);
                }}
                style={{
                  border: 'none',
                  background: 'transparent',
                  cursor: 'pointer',
                  padding: 0,
                  margin: 0,
                }}
              />
            ))}
          </div>
        )}
      </div>
      {showValue && (
        <span
          style={{
            minWidth: 24,
            fontSize: preset.valueFont,
            color: value ? 'var(--text)' : 'color-mix(in srgb, var(--text-muted) 72%, transparent)',
            fontVariantNumeric: 'tabular-nums',
            textAlign: 'right',
            fontWeight: value ? 700 : 500,
          }}
        >
          {value ? value.toFixed(1).replace(/\.0$/, '') : '-'}
        </span>
      )}
    </div>
  );
}
