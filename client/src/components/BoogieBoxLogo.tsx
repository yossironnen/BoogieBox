import { useId, type CSSProperties } from 'react';

/** The BoogieBox mark (hand-drawn boombox). Coloured by `currentColor`, so it follows the theme accent. */
export default function BoogieBoxLogo({ size = 22, style }: { size?: number; style?: CSSProperties }) {
  const id = `wob-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`;
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" style={style} aria-hidden="true" data-testid="boogiebox-logo">
      <defs>
        <filter id={id} x="-15%" y="-15%" width="130%" height="130%">
          <feTurbulence type="fractalNoise" baseFrequency="0.22" numOctaves={2} seed={4} result="n" />
          <feDisplacementMap in="SourceGraphic" in2="n" scale={1.8} xChannelSelector="R" yChannelSelector="G" />
        </filter>
      </defs>
      <g filter={`url(#${id})`} fill="none" stroke="currentColor" strokeWidth={2.6} strokeLinecap="round" strokeLinejoin="round">
        <path d="M9.5 12.5V10Q9.5 8.5 11 8.5H21Q22.5 8.5 22.5 10V12.5" />
        <rect x="3.4" y="12.4" width="25.2" height="15.2" rx="4" />
        <circle cx="10.5" cy="20" r="3.4" fill="currentColor" />
        <circle cx="21.5" cy="20" r="3.4" fill="currentColor" />
      </g>
    </svg>
  );
}
