/**
 * Provides one deliberate, touch-safe confirmation surface for mobile actions.
 */

import React, { useEffect, useState } from 'react';
import MobileBottomSheet from './MobileBottomSheet';

/** Mobile Confirmation Sheet is part of this module's public API. */
export default function MobileConfirmationSheet({
  open,
  title,
  description,
  itemLabel,
  confirmLabel,
  busyLabel = 'Working…',
  errorMessage = 'Could not complete this action.',
  onClose,
  onConfirm,
}: {
  open: boolean;
  title: string;
  description: string;
  itemLabel?: string;
  confirmLabel: string;
  busyLabel?: string;
  errorMessage?: string;
  onClose: () => void;
  onConfirm: () => Promise<unknown> | unknown;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open) {
      setBusy(false);
      setError('');
    }
  }, [open]);

  if (!open) return null;

  const requestClose = () => {
    if (!busy) onClose();
  };

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await onConfirm();
      onClose();
    } catch (confirmError: any) {
      setError(confirmError?.message || errorMessage);
      setBusy(false);
    }
  };

  return (
    <MobileBottomSheet
      title={title}
      onClose={requestClose}
      closeLabel={`Cancel ${title}`}
      dialogRole="alertdialog"
    >
      <div style={styles.content}>
        <div aria-hidden="true" style={styles.warningMark}>!</div>
        <p style={styles.description}>{description}</p>
        {itemLabel ? <div style={styles.itemLabel}>{itemLabel}</div> : null}
        {error ? <div role="alert" style={styles.error}>{error}</div> : null}
        <div style={styles.actions}>
          <button
            type="button"
            disabled={busy}
            style={styles.cancelButton}
            onClick={requestClose}
          >
            Keep it
          </button>
          <button
            type="button"
            disabled={busy}
            style={{ ...styles.confirmButton, ...(busy ? styles.disabled : null) }}
            onClick={() => void confirm()}
          >
            {busy ? busyLabel : confirmLabel}
          </button>
        </div>
      </div>
    </MobileBottomSheet>
  );
}

const styles: Record<string, React.CSSProperties> = {
  content: {
    display: 'grid',
    gap: 12,
  },
  warningMark: {
    width: 44,
    height: 44,
    display: 'grid',
    placeItems: 'center',
    border: '1px solid color-mix(in srgb, var(--danger) 32%, var(--divider-subtle))',
    borderRadius: 13,
    background: 'color-mix(in srgb, var(--danger) 9%, var(--surface))',
    color: 'var(--danger)',
    fontSize: 20,
    fontWeight: 850,
  },
  description: {
    margin: 0,
    color: 'var(--text-muted)',
    fontSize: 14,
    lineHeight: 1.55,
  },
  itemLabel: {
    overflow: 'hidden',
    padding: '11px 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontSize: 13,
    fontWeight: 750,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  error: {
    padding: '10px 12px',
    border: '1px solid color-mix(in srgb, var(--danger) 32%, var(--divider-subtle))',
    borderRadius: 11,
    background: 'color-mix(in srgb, var(--danger) 8%, var(--surface))',
    color: 'var(--danger)',
    fontSize: 12,
    fontWeight: 650,
  },
  actions: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 8,
  },
  cancelButton: {
    minWidth: 0,
    minHeight: 52,
    padding: '0 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 12,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  confirmButton: {
    minWidth: 0,
    minHeight: 52,
    padding: '0 12px',
    border: '1px solid color-mix(in srgb, var(--danger) 42%, var(--divider-subtle))',
    borderRadius: 12,
    background: 'color-mix(in srgb, var(--danger) 12%, var(--surface))',
    color: 'var(--danger)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 800,
  },
  disabled: {
    cursor: 'default',
    opacity: 0.55,
  },
};
