/**
 * Play On glyphs: the "devices" output icon (monitor + speaker, unlike the volume icon), the local laptop/phone
 * rows and one generic speaker cabinet used for every renderer (never per-brand).
 */

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

export function OutputIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="2" y="4" width="12" height="9" rx="1.5" />
      <path d="M6 17h4" />
      <rect x="16" y="6" width="6" height="12" rx="1.5" />
      <circle cx="19" cy="14" r="1.2" />
    </svg>
  );
}

export function BrowserIcon({ size = 20, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M2 20h20" />
    </svg>
  );
}

export function PhoneIcon({ size = 20, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="7" y="2" width="10" height="20" rx="2" />
      <path d="M11 18h2" />
    </svg>
  );
}

export function RendererIcon({ size = 20, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <rect x="6" y="2" width="12" height="20" rx="2" />
      <circle cx="12" cy="14" r="4" />
      <circle cx="12" cy="6.5" r="1" />
    </svg>
  );
}

export function CheckIcon({ size = 16, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke} strokeWidth={2.4}>
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

export function UserIcon({ size = 12, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21v-1a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1" />
    </svg>
  );
}

export function RefreshIcon({ size = 15, style }: IconProps) {
  return (
    <svg width={size} height={size} style={style} {...stroke}>
      <path d="M21 12a9 9 0 1 1-2.64-6.36" />
      <polyline points="21 3 21 9 15 9" />
    </svg>
  );
}
