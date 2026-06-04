/**
 * Defines mobile Mobile Bottom Sheet behavior for the BoogieBox React client.
 */

import React from 'react';

/** Mobile Bottom Sheet is part of this module's public API. */
export default function MobileBottomSheet({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <div style={styles.overlay} role="dialog" aria-modal="true" aria-label={title}>
      <button type="button" style={styles.scrim} onClick={onClose} aria-label="Close sheet" />
      <section style={styles.sheet}>
        <div style={styles.handle} />
        <header style={styles.header}>
          <h2 style={styles.title}>{title}</h2>
          <button type="button" style={styles.close} onClick={onClose} aria-label="Close">x</button>
        </header>
        {children}
      </section>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: 'fixed', inset: 0, zIndex: 90 },
  scrim: { position: 'absolute', inset: 0, border: 'none', background: 'rgba(0,0,0,0.54)' },
  sheet: {
    position: 'absolute',
    left: 0,
    right: 0,
    bottom: 0,
    maxHeight: '84dvh',
    overflowY: 'auto',
    padding: '10px 16px calc(env(safe-area-inset-bottom, 0px) + 18px)',
    borderTop: '1px solid var(--border)',
    borderRadius: '20px 20px 0 0',
    background: 'var(--surface)',
    color: 'var(--text)',
  },
  handle: { width: 42, height: 4, borderRadius: 999, background: 'var(--border)', margin: '0 auto 14px' },
  header: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 },
  title: { margin: 0, fontSize: 20, fontWeight: 800 },
  close: { minWidth: 44, minHeight: 44, border: '1px solid var(--border)', borderRadius: 14, background: 'transparent', color: 'var(--text)', fontFamily: 'inherit', fontSize: 18 },
};
