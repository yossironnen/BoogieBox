/**
 * Defines mobile Mobile Skeleton behavior for the BoogieBox React client.
 */

import React from 'react';

/** Mobile Skeleton is part of this module's public API. */
export default function MobileSkeleton({
  variant = 'row',
  count = 4,
}: {
  variant?: 'row' | 'album' | 'poster';
  count?: number;
}) {
  return (
    <div style={variant === 'row' ? styles.rows : styles.grid} aria-hidden="true">
      {Array.from({ length: count }).map((_, index) => (
        <span
          key={index}
          style={{
            ...styles.item,
            ...(variant === 'poster' ? styles.poster : variant === 'album' ? styles.album : styles.row),
          }}
        />
      ))}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  rows: { display: 'grid', gap: 10 },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 10 },
  item: {
    borderRadius: 14,
    background: 'linear-gradient(90deg, color-mix(in srgb, var(--surface) 86%, var(--bg)), color-mix(in srgb, var(--surface) 70%, var(--accent)), color-mix(in srgb, var(--surface) 86%, var(--bg)))',
    backgroundSize: '220% 100%',
    opacity: 0.62,
  },
  row: { height: 72 },
  album: { aspectRatio: '1 / 1' },
  poster: { aspectRatio: '2 / 3' },
};
