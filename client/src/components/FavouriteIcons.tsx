/**
 * Icons for Favourites (heart) and Internet radio, shared by Browse, Home, context menus and mobile views.
 */

import React from 'react';

const HEART_PATH = 'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 0 0-7.8 7.8l1 1.1L12 21l7.8-7.5 1-1.1a5.5 5.5 0 0 0 0-7.8z';

/** Heart: outlined, or filled when `filled`. Inherits `currentColor`. */
export function HeartIcon({ size = 14, filled = false }: { size?: number; filled?: boolean }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? 'currentColor' : 'none'}
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ flexShrink: 0 }}
    >
      <path d={HEART_PATH} />
    </svg>
  );
}

/** Radio waves around a mast. */
export function RadioIcon({ size = 14 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ flexShrink: 0 }}
    >
      <circle cx="12" cy="12" r="2" />
      <path d="M16.2 7.8a6 6 0 0 1 0 8.4" />
      <path d="M7.8 16.2a6 6 0 0 1 0-8.4" />
      <path d="M19.1 4.9a10 10 0 0 1 0 14.2" />
      <path d="M4.9 19.1a10 10 0 0 1 0-14.2" />
    </svg>
  );
}

/** Music note (songs). */
export function SongIcon({ size = 14 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ flexShrink: 0 }}
    >
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </svg>
  );
}

/** A heart toggle button for a favourite. Stops the click so it never triggers the row/card underneath. */
export function HeartToggle({
  on,
  onToggle,
  size = 15,
  label,
  style,
}: {
  on: boolean;
  onToggle: () => void;
  size?: number;
  /** What is being hearted, for the accessible label. */
  label: string;
  style?: React.CSSProperties;
}) {
  const text = on ? `Remove ${label} from Favourites` : `Add ${label} to Favourites`;
  return (
    <button
      type="button"
      aria-pressed={on}
      aria-label={text}
      title={text}
      onClick={(e) => { e.stopPropagation(); e.preventDefault(); onToggle(); }}
      onMouseDown={(e) => e.stopPropagation()}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 4,
        border: 'none',
        borderRadius: 6,
        background: 'transparent',
        cursor: 'pointer',
        color: on ? 'var(--accent)' : 'var(--text-muted)',
        ...style,
      }}
    >
      <HeartIcon size={size} filled={on} />
    </button>
  );
}
