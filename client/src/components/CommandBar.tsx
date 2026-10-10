// Shared single-row header for the browse screens (Browse root, Browse
// drill-down, Search): screen controls on the left, optional icon actions on
// the right, and a decorative screen-name watermark behind everything.
// Library totals live in the sidebar status panel. See
// wip/browse-command-bar-plan.md.
import React from 'react';

export interface CommandBarProps {
  /** Screen name drawn as the decorative watermark ("Browse", "Search"). */
  watermark: string;
  /** Left zone: tabs, chips, breadcrumb, search input… */
  children: React.ReactNode;
  /** Right zone after the divider: icon buttons. */
  actions?: React.ReactNode;
  style?: React.CSSProperties;
}

export default function CommandBar({ watermark, children, actions, style }: CommandBarProps) {
  return (
    <div style={{ ...styles.bar, ...style }} data-command-bar={watermark.toLowerCase()}>
      <div aria-hidden="true" style={styles.watermarkLayer}>
        <span style={styles.watermark} data-testid="command-bar-watermark">{watermark}</span>
      </div>
      <div style={styles.left}>{children}</div>
      {actions && (
        <div style={styles.actions}>
          <span style={styles.divider} />
          {actions}
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  bar: {
    position: 'relative',
    // `isolation` makes the bar its own stacking context, so popovers anchored
    // inside it can only rise above the content below if the bar itself does.
    isolation: 'isolate',
    zIndex: 30,
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'nowrap',
    gap: 10,
    minHeight: 56,
    padding: '8px 20px',
    boxSizing: 'border-box',
    flexShrink: 0,
    borderBottom: '1px solid color-mix(in srgb, var(--border) 62%, transparent)',
    background: 'color-mix(in srgb, var(--surface) 26%, transparent)',
  },
  // Clipping lives on this layer, not on the bar: the bar must stay
  // overflow-visible so popovers anchored inside it are not cut off.
  watermarkLayer: {
    position: 'absolute',
    inset: 0,
    overflow: 'hidden',
    pointerEvents: 'none',
    zIndex: 0,
  },
  watermark: {
    position: 'absolute',
    left: '50%',
    top: '50%',
    transform: 'translate(-50%, -50%)',
    // Cap height (~70px) is taller than the 56px bar, so the layer trims the
    // word top and bottom on purpose.
    fontSize: 100,
    fontWeight: 800,
    lineHeight: 1,
    letterSpacing: -3.5,
    whiteSpace: 'nowrap',
    color: 'color-mix(in srgb, var(--accent) 35%, var(--text))',
    opacity: 0.12,
    userSelect: 'none',
  },
  left: {
    position: 'relative',
    zIndex: 1,
    flex: '1 1 auto',
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 10,
  },
  actions: {
    position: 'relative',
    zIndex: 1,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  },
  divider: {
    width: 1,
    height: 22,
    background: 'var(--border)',
    margin: '0 4px',
  },
};
