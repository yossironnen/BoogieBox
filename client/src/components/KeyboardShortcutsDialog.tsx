/**
 * "Keyboard shortcuts" help dialog, opened with `?` (same overlay/card shell as ConfirmModal).
 */

import React, { useEffect, useRef } from 'react';
import { PLAYER_SHORTCUTS } from '../hooks/usePlayerShortcuts';

function KeyboardIcon() {
  return (
    <svg width={18} height={18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ color: 'var(--accent)', opacity: 0.85 }}>
      <rect x="2" y="6" width="20" height="12" rx="2" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" />
    </svg>
  );
}

export default function KeyboardShortcutsDialog({ onClose }: { onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeRef.current?.focus();
    return () => returnFocus?.focus();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  return (
    <div role="presentation" style={S.overlay} onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="keyboard-shortcuts-title" style={S.card}>
        <div style={S.header}>
          <KeyboardIcon />
          <div id="keyboard-shortcuts-title" style={S.title}>Keyboard shortcuts</div>
        </div>
        <dl style={S.list}>
          {PLAYER_SHORTCUTS.map((row) => (
            <div key={row.keys} style={S.row}>
              <dt style={S.label}>{row.label}</dt>
              <dd style={S.keys}><kbd style={S.kbd}>{row.keys}</kbd></dd>
            </div>
          ))}
        </dl>
        <div style={S.footer}>
          <button ref={closeRef} type="button" style={S.close} onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  overlay: {
    position: 'fixed', inset: 0, zIndex: 2000,
    background: 'rgba(0,0,0,0.65)',
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    padding: 24,
  },
  card: {
    width: '100%', maxWidth: 420,
    background: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 12,
    overflow: 'hidden',
    color: 'var(--text)',
  },
  header: { display: 'flex', alignItems: 'center', gap: 10, padding: '18px 24px 6px' },
  title: { fontSize: 18, fontWeight: 700 },
  list: { margin: 0, padding: '8px 24px 16px', display: 'flex', flexDirection: 'column', gap: 8 },
  row: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 },
  label: { fontSize: 14, color: 'var(--text-muted)' },
  keys: { margin: 0 },
  kbd: {
    display: 'inline-block',
    padding: '2px 8px',
    border: '1px solid var(--border)',
    borderRadius: 6,
    background: 'var(--bg)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 650,
    whiteSpace: 'nowrap',
  },
  footer: { padding: '14px 24px', borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'flex-end' },
  close: {
    padding: '8px 18px', background: 'var(--accent)', border: 'none', borderRadius: 6,
    color: 'var(--on-accent, #fff)', fontSize: 15, fontWeight: 700, cursor: 'pointer', fontFamily: 'inherit',
  },
};
