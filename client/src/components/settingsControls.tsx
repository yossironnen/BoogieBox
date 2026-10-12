/**
 * Shared Settings controls: switch, titled toggle row and the inline busy/result line.
 */

import React from 'react';
import { hybridControlStyles } from '../hybridPreview';

export function ToggleSwitch({
  checked,
  onChange,
  ariaLabel,
  disabled,
}: {
  checked: boolean;
  onChange: () => void;
  ariaLabel: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={ariaLabel}
      aria-checked={checked}
      onClick={onChange}
      disabled={disabled}
      style={{
        ...hybridControlStyles.switchTrack,
        ...(checked ? hybridControlStyles.switchTrackActive : {}),
        ...(disabled ? { opacity: 0.6, cursor: 'default' } : {}),
        flexShrink: 0,
      }}
    >
      <span style={{ ...hybridControlStyles.switchThumb, ...(checked ? hybridControlStyles.switchThumbActive : {}) }} />
    </button>
  );
}

export function ToggleRow({
  title,
  description,
  checked,
  onChange,
  ariaLabel,
  disabled,
}: {
  title: string;
  description: React.ReactNode;
  checked: boolean;
  onChange: () => void;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 20, marginBottom: 16 }}>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>{title}</div>
        <div style={{ fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 }}>{description}</div>
      </div>
      <ToggleSwitch checked={checked} onChange={onChange} ariaLabel={ariaLabel ?? title} disabled={disabled} />
    </div>
  );
}

/** Renders a card's busy/result feedback line consistently: muted while busy,
 * success-toned otherwise, danger-toned for a result starting with "Error". */
export function InlineStatus({ busy, busyText, result }: { busy: boolean; busyText: string; result: string | null }) {
  if (busy) return <div style={{ fontSize: 14, color: 'var(--text-muted)', marginTop: 6 }}>{busyText}</div>;
  if (!result) return null;
  const isError = result.startsWith('Error') || result.startsWith('✗');
  return <div style={{ fontSize: 14, color: isError ? 'var(--danger)' : 'var(--success)', marginTop: 6 }}>{result}</div>;
}
