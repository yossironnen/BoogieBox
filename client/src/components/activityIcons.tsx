/**
 * Glyphs for the admin Activity page and its sidebar indicator. Device icons come from `remote/OutputIcon`
 * (browser, phone, speaker) and `FavouriteIcons` (radio); these are the ones nothing else had.
 */

import React from 'react';

type IconProps = { size?: number; style?: React.CSSProperties };

const stroke = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
};

/** Pulse line: the Activity page, nav item and sidebar indicator. */
export function ActivityIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M3 12h4l3-7 4 14 3-7h4" />
    </svg>
  );
}

export function PlayIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M7 4v16l13-8z" />
    </svg>
  );
}

export function PauseIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="6" y="4" width="4" height="16" rx="1" />
      <rect x="14" y="4" width="4" height="16" rx="1" />
    </svg>
  );
}

export function DownloadIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M5 21h14" />
    </svg>
  );
}

/** Same bars as the Mixes nav icon. */
export function MixJobIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="3" y="4" width="18" height="4" rx="1" />
      <rect x="3" y="10" width="12" height="4" rx="1" />
      <rect x="3" y="16" width="15" height="4" rx="1" />
    </svg>
  );
}

export function ServerJobsIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="3" y="4" width="18" height="6" rx="1.5" />
      <rect x="3" y="14" width="18" height="6" rx="1.5" />
      <path d="M7 7h.01M7 17h.01" />
    </svg>
  );
}

export function TranscodeIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M4 7h11l-3-3" />
      <path d="M20 17H9l3 3" />
    </svg>
  );
}

export function UsersIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <circle cx="9" cy="8" r="3.5" />
      <path d="M2.5 20a6.5 6.5 0 0 1 13 0" />
      <path d="M16 4.5a3.5 3.5 0 0 1 0 7" />
      <path d="M18.5 14a6.5 6.5 0 0 1 3 6" />
    </svg>
  );
}

/** A TV: DLNA renderers pulling from the DLNA server. */
export function TvIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="2" y="5" width="20" height="13" rx="2" />
      <path d="M8 21h8" />
    </svg>
  );
}

export function ClockIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </svg>
  );
}

export function CancelIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </svg>
  );
}
