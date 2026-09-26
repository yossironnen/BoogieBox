/**
 * Vintage style picker: one preview card per registered vintage style
 * (Settings → User Settings, shown while the Vintage theme mode is active).
 */

import React from 'react';
import { VINTAGE_STYLE_IDS, VINTAGE_STYLES, type VintageStyle, type VintageStyleDefinition } from '../vintageThemes';

const ICON_PROPS = {
  width: 20, height: 20, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor',
  strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round', 'aria-hidden': true,
} as const;

function VintageStyleIcon({ style }: { style: VintageStyle }) {
  switch (style) {
    case 'recordshop':
    default:
      // Record with a label and a groove highlight.
      return <svg {...ICON_PROPS}><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="3" /><path d="M12 3a9 9 0 0 1 9 9" /></svg>;
  }
}

function CheckBadge() {
  return (
    <span style={S.check} aria-hidden="true">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-10" /></svg>
    </span>
  );
}

function MiniPreview({ def }: { def: VintageStyleDefinition }) {
  const p = def.preview;
  const tiles = [...def.stripes, ...def.stickerShadows].slice(0, 6);
  return (
    <div style={{ ...S.preview, background: p.bg }} aria-hidden="true">
      <div style={{ ...S.previewTop, background: p.top }} />
      <div style={S.previewBody}>
        <div style={{ ...S.previewSide, background: p.side }}>
          <div style={{ ...S.previewSideItem, background: p.accent }} />
          <div style={{ ...S.previewSideItem, background: p.sideItem }} />
          <div style={{ ...S.previewSideItem, background: p.sideItem }} />
          <div style={{ ...S.previewSideItem, background: p.sideItem }} />
        </div>
        <div style={S.previewGrid}>
          {tiles.map((color, index) => (
            <div key={`${color}-${index}`} style={{ ...S.previewTile, background: color }} />
          ))}
        </div>
      </div>
      <div style={{ ...S.previewBar, background: p.bar }}>
        <div style={{ ...S.previewMeter, background: p.meter }} />
        <div style={{ ...S.previewMeter, background: p.meter }} />
        <div style={{ ...S.previewProgress, background: p.accent }} />
      </div>
    </div>
  );
}

/** Vintage Style Picker is part of this module's public API. */
export default function VintageStylePicker({
  value,
  onChange,
}: {
  value: VintageStyle;
  onChange: (style: VintageStyle) => void;
}) {
  return (
    <div role="group" aria-label="Vintage style" style={S.grid}>
      {VINTAGE_STYLE_IDS.map((id) => {
        const def = VINTAGE_STYLES[id];
        const active = value === id;
        return (
          <button
            key={id}
            type="button"
            aria-pressed={active}
            aria-label={`Use ${def.name} vintage style`}
            onClick={() => onChange(id)}
            style={{ ...S.card, ...(active ? S.cardActive : {}) }}
          >
            <MiniPreview def={def} />
            <span style={S.meta}>
              <span style={{ ...S.icon, color: def.stripes[1] ?? 'var(--accent)' }}>
                <VintageStyleIcon style={id} />
              </span>
              <span style={S.text}>
                <span style={S.name}>{def.name}</span>
                <span style={S.era}>{def.era}</span>
              </span>
              {active && <CheckBadge />}
            </span>
          </button>
        );
      })}
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  grid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 260px))',
    gap: 14,
  },
  card: {
    display: 'flex',
    flexDirection: 'column',
    padding: 0,
    overflow: 'hidden',
    textAlign: 'left',
    cursor: 'pointer',
    borderRadius: 12,
    border: '2px solid var(--divider-subtle)',
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
  },
  cardActive: {
    borderColor: 'var(--accent)',
  },
  preview: {
    height: 120,
    display: 'flex',
    flexDirection: 'column',
  },
  previewTop: { height: 14 },
  previewBody: { flex: 1, display: 'flex', minHeight: 0 },
  previewSide: {
    width: 40,
    display: 'flex',
    flexDirection: 'column',
    gap: 5,
    padding: '7px 6px',
    boxSizing: 'border-box',
  },
  previewSideItem: { height: 7, borderRadius: 2 },
  previewGrid: {
    flex: 1,
    display: 'grid',
    gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
    gap: 5,
    padding: 8,
    alignContent: 'start',
  },
  previewTile: { aspectRatio: '1', borderRadius: 2 },
  previewBar: {
    height: 26,
    display: 'flex',
    alignItems: 'center',
    gap: 5,
    padding: '0 8px',
  },
  previewMeter: { width: 26, height: 14, borderRadius: 2 },
  previewProgress: { flex: 1, height: 3, opacity: 0.85 },
  meta: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    padding: '11px 12px',
  },
  icon: { display: 'flex', flexShrink: 0 },
  text: { display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 },
  name: { fontSize: 14, fontWeight: 650 },
  era: { fontSize: 12, color: 'var(--text-muted)' },
  check: {
    marginLeft: 'auto',
    width: 22,
    height: 22,
    borderRadius: '50%',
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
  },
};
