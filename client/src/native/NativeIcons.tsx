/**
 * Icons for the desktop app's Hi-Res output (picker row, dock badge, Audio output settings), drawn as in the approved
 * mockup. Stroke icons inherit `currentColor`.
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

/** "This PC · Hi-Res": a monitor with level bars. */
export function DacIcon({ size = 20, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="2" y="4" width="14" height="10" rx="2" />
      <path d="M6 18h6" />
      <path d="M9 14v4" />
      <path d="M19 7v10" />
      <path d="M22 9v6" />
    </svg>
  );
}

/** Bit-perfect, and the Mode heading. */
export function WaveIcon({ size = 14, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M2 12h3l3-7 4 14 3-7h7" />
    </svg>
  );
}

export function VolumeIcon({ size = 14, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M11 5 6 9H2v6h4l5 4V5Z" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
    </svg>
  );
}

/** Resampled, and the nearest-rate switch. */
export function SineIcon({ size = 14, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M3 12c2-6 4-6 6 0s4 6 6 0 4-6 6 0" />
    </svg>
  );
}

/** Shared mode. */
export function SharedIcon({ size = 14, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <circle cx="9" cy="12" r="5" />
      <circle cx="15" cy="12" r="5" />
    </svg>
  );
}

/** Converted by server. */
export function ServerIcon({ size = 14, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M8 20h8" />
      <path d="m9 10 2 2 4-4" />
    </svg>
  );
}

/** Mixed down to stereo. */
export function DownmixIcon({ size = 14, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M5 21h14" />
    </svg>
  );
}

/** Exclusive mode. */
export function LockIcon({ size = 18, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="5" y="11" width="14" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
    </svg>
  );
}

/** Device heading. */
export function SpeakerIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M11 5 6 9H2v6h4l5 4V5Z" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
      <path d="M19 5a10 10 0 0 1 0 14" />
    </svg>
  );
}

/** Buffer heading. */
export function TimerIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <circle cx="12" cy="13" r="8" />
      <path d="M12 9v4l2 2" />
      <path d="M9 2h6" />
    </svg>
  );
}

export const BADGE_ICONS = {
  bitPerfect: WaveIcon,
  volume: VolumeIcon,
  resampled: SineIcon,
  shared: SharedIcon,
  server: ServerIcon,
  downmix: DownmixIcon,
} as const;
