/**
 * Defines mobile Mobile Bottom Sheet behavior for the BoogieBox React client.
 */

import React, { useEffect, useId, useRef } from 'react';

/** Mobile Bottom Sheet is part of this module's public API. */
export default function MobileBottomSheet({
  title,
  onClose,
  children,
  closeLabel = 'Close',
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  closeLabel?: string;
}) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    returnFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const focusTimer = window.setTimeout(() => closeRef.current?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onCloseRef.current();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.clearTimeout(focusTimer);
      window.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = previousOverflow;
      returnFocusRef.current?.focus();
    };
  }, []);

  return (
    <div style={styles.overlay}>
      <button
        type="button"
        tabIndex={-1}
        style={styles.scrim}
        onClick={onClose}
        aria-label={`Dismiss ${title}`}
      />
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        style={styles.sheet}
      >
        <div aria-hidden="true" style={styles.handle} />
        <header style={styles.header}>
          <div>
            <div style={styles.eyebrow}>BoogieBox</div>
            <h2 id={titleId} style={styles.title}>{title}</h2>
          </div>
          <button
            ref={closeRef}
            type="button"
            style={styles.close}
            onClick={onClose}
            aria-label={closeLabel}
          >
            ×
          </button>
        </header>
        <div style={styles.body}>{children}</div>
      </section>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: {
    position: 'fixed',
    inset: 0,
    zIndex: 1200,
    display: 'flex',
    alignItems: 'flex-end',
  },
  scrim: {
    position: 'absolute',
    inset: 0,
    border: 'none',
    background: 'var(--overlay)',
  },
  sheet: {
    width: '100%',
    maxHeight: '84dvh',
    position: 'relative',
    display: 'grid',
    gridTemplateRows: 'auto auto minmax(0, 1fr)',
    boxSizing: 'border-box',
    overflow: 'hidden',
    padding: '10px 16px 0',
    borderTop: '1px solid var(--border-strong)',
    borderRadius: '20px 20px 0 0',
    background: 'var(--surface-raised)',
    color: 'var(--text)',
    boxShadow: 'var(--shadow-raised)',
  },
  handle: {
    width: 38,
    height: 4,
    margin: '0 auto 10px',
    borderRadius: 999,
    background: 'var(--border-strong)',
  },
  header: {
    minHeight: 52,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    paddingBottom: 10,
    borderBottom: '1px solid var(--divider-subtle)',
  },
  eyebrow: {
    color: 'var(--accent)',
    fontSize: 9,
    fontWeight: 800,
    letterSpacing: 1.1,
    textTransform: 'uppercase',
  },
  title: {
    margin: '3px 0 0',
    color: 'var(--text)',
    fontSize: 19,
    fontWeight: 800,
    letterSpacing: -0.4,
    lineHeight: 1.05,
  },
  close: {
    width: 44,
    minWidth: 44,
    height: 44,
    display: 'grid',
    placeItems: 'center',
    padding: 0,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 12,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 20,
  },
  body: {
    minHeight: 0,
    display: 'grid',
    gap: 12,
    overflowY: 'auto',
    overscrollBehaviorY: 'contain',
    padding: '14px 0 calc(env(safe-area-inset-bottom, 0px) + 18px)',
  },
};
