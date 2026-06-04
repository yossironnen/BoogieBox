/**
 * Defines mobile Mobile Progress Bar behavior for the BoogieBox React client.
 */

import React from 'react';

/** Mobile Progress Bar is part of this module's public API. */
export default function MobileProgressBar({
  value,
  max,
  height = 4,
}: {
  value?: number | null;
  max?: number | null;
  height?: number;
}) {
  const pct = max && max > 0 ? Math.max(0, Math.min(100, ((value ?? 0) / max) * 100)) : 0;
  if (pct <= 0) return null;
  return (
    <span style={{ ...styles.track, height }}>
      <span style={{ ...styles.fill, width: `${pct}%` }} />
    </span>
  );
}

const styles: Record<string, React.CSSProperties> = {
  track: {
    display: 'block',
    width: '100%',
    borderRadius: 999,
    background: 'color-mix(in srgb, var(--text-muted) 18%, transparent)',
    overflow: 'hidden',
  },
  fill: {
    display: 'block',
    height: '100%',
    borderRadius: 999,
    background: 'var(--accent)',
  },
};
