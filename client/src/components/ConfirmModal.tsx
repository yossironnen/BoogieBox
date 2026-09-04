/**
 * Defines the Confirm Modal React component — the app's one themed
 * replacement for the native `window.confirm`/`window.alert` popups, which
 * render with the browser's own chrome and cannot be restyled: no app theme,
 * no accent color, a generic "This site says" header. Every confirm/alert
 * dialog in the app should render through this component instead, so a
 * decision prompt always matches the rest of the UI (see the other modals in
 * this folder, e.g. MergeArtistsModal, for the same overlay/card shell).
 *
 * Usage: keep a single `useState<ConfirmRequest | null>` (or similar) per
 * component for "the confirm currently open", mount `<ConfirmModal .../>`
 * only while it is set, and clear it from `onCancel`/`onConfirm`. For a
 * single-button info dialog (replacing `window.alert`), pass
 * `cancelLabel={null}` and use the same handler for `onConfirm`/`onCancel`.
 */

import React, { useEffect, useRef } from 'react';

interface Props {
  /** Eyebrow label above the title, e.g. "Remove Library". Also the accessible dialog name. */
  title: string;
  /** Body copy. Supports `\n` line breaks (rendered via `white-space: pre-line`). */
  message: React.ReactNode;
  /** Defaults to 'Confirm'. */
  confirmLabel?: string;
  /** Defaults to 'Cancel'. Pass `null` for a single-button info/acknowledgement dialog. */
  cancelLabel?: string | null;
  /** 'danger' colors the eyebrow and the confirm button for a destructive action. */
  tone?: 'default' | 'danger';
  /** Disables both buttons and swaps the confirm button's label while an action is in flight. */
  busy?: boolean;
  /** Shown on the confirm button while `busy` is true. */
  busyLabel?: string;
  onConfirm: () => void;
  /** Also fires on Escape and on a backdrop click. */
  onCancel: () => void;
}

/** Confirm Modal is part of this module's public API. */
export default function ConfirmModal({
  title,
  message,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  tone = 'default',
  busy = false,
  busyLabel,
  onConfirm,
  onCancel,
}: Props) {
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    confirmRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !busy) onCancel();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [busy, onCancel]);

  // `--danger`/`--on-accent` are only set while the hybrid desktop theme is
  // active (App.tsx applies them to <html> conditionally); fall back to fixed
  // colors so the dialog still reads correctly on mobile or before that runs.
  const accentColor = tone === 'danger' ? 'var(--danger, #dc2626)' : 'var(--accent)';

  return (
    <div
      role="presentation"
      style={{
        position: 'fixed', inset: 0, zIndex: 2000,
        background: 'rgba(0,0,0,0.65)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 24,
      }}
      onClick={e => { if (e.target === e.currentTarget && !busy) onCancel(); }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-modal-title"
        aria-describedby="confirm-modal-message"
        style={{
          width: '100%', maxWidth: 440,
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 12,
          display: 'flex', flexDirection: 'column',
          overflow: 'hidden',
          fontFamily: 'var(--font), monospace',
          color: 'var(--text)',
        }}
      >
        {/* Header */}
        <div style={{ padding: '18px 24px 4px' }}>
          <div style={{ fontSize: 9, fontWeight: 700, letterSpacing: 1.4, textTransform: 'uppercase', color: accentColor, marginBottom: 3 }}>
            {tone === 'danger' ? 'Confirm — this cannot be undone' : 'Confirm'}
          </div>
          <div id="confirm-modal-title" style={{ fontSize: 16, fontWeight: 700 }}>{title}</div>
        </div>

        {/* Body */}
        <div style={{ padding: '14px 24px 20px' }}>
          <div id="confirm-modal-message" style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.6, whiteSpace: 'pre-line' }}>
            {message}
          </div>
        </div>

        {/* Footer */}
        <div style={{ padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'flex-end', gap: 10 }}>
          {cancelLabel !== null && (
            <button
              onClick={onCancel}
              disabled={busy}
              style={{
                padding: '8px 18px', background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 6,
                color: 'var(--text)', fontSize: 13, cursor: busy ? 'not-allowed' : 'pointer',
                opacity: busy ? 0.6 : 1, fontFamily: 'var(--font), monospace',
              }}
            >
              {cancelLabel}
            </button>
          )}
          <button
            ref={confirmRef}
            onClick={onConfirm}
            disabled={busy}
            style={{
              padding: '8px 18px',
              background: busy ? `color-mix(in srgb, ${accentColor} 50%, var(--surface))` : accentColor,
              border: 'none', borderRadius: 6,
              color: 'var(--on-accent, #fff)', fontSize: 13, fontWeight: 700,
              cursor: busy ? 'not-allowed' : 'pointer',
              fontFamily: 'var(--font), monospace',
            }}
          >
            {busy ? (busyLabel ?? confirmLabel) : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
