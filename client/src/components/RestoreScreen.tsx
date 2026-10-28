/**
 * Full-window screens shown while a backup is restored ("Restoring a backup…") and after it ("Restore
 * complete"). A browser that didn't start the restore waits for the server and then reloads into sign-in.
 */

import React, { useEffect, useState } from 'react';
import { api } from '../api';
import type { RestoreOverlay } from '../restoreState';
import { BackupIcons as I, formatBackupDate } from './backupUi';
import { hybridControlStyles } from '../hybridPreview';

const POLL_MS = 1500;

function reload() {
  window.location.reload();
}

export default function RestoreScreen({ overlay, onReload = reload }: { overlay: RestoreOverlay; onReload?: () => void }) {
  const [step, setStep] = useState<string | null>(null);
  const restoring = overlay.phase === 'restoring';
  const startedHere = overlay.phase === 'restoring' && overlay.startedHere;

  useEffect(() => {
    if (!restoring) return undefined;
    let cancelled = false;
    const poll = async () => {
      try {
        const status = await api.systemStatus();
        if (cancelled) return;
        setStep(status.restoreStep ?? null);
        if (!status.restoring && !startedHere) onReload();
      } catch {
        // The server may be briefly unreachable; keep waiting.
      }
    };
    void poll();
    const id = window.setInterval(() => { void poll(); }, POLL_MS);
    return () => { cancelled = true; window.clearInterval(id); };
  }, [onReload, restoring, startedHere]);

  return (
    <div style={S.viewport}>
      {overlay.phase === 'restoring' ? (
        <section aria-busy="true" aria-labelledby="restoring-title" style={S.card}>
          <span style={{ color: 'var(--accent)', opacity: 0.85 }}><I.Restore size={40} /></span>
          <h1 id="restoring-title" style={S.title}>Restoring a backup…</h1>
          <p style={S.text}>BoogieBox is unavailable for a moment.</p>
          <div style={S.track}><div style={S.bar} /></div>
          {step && <div role="status" style={{ fontSize: 13, color: 'var(--text-muted)' }}>{step}…</div>}
        </section>
      ) : (
        <section aria-labelledby="done-title" style={S.card}>
          <I.CheckCircle size={40} color="var(--success, #65c590)" />
          <h1 id="done-title" style={S.title}>Restore complete</h1>
          <p style={S.text}>
            {overlay.backupDate
              ? `The backup from ${formatBackupDate(overlay.backupDate, { weekday: false })} is in place. Sign in again to continue.`
              : 'The backup is in place. Sign in again to continue.'}
          </p>
          <button type="button" style={{ ...hybridControlStyles.primaryButton, width: '100%', minHeight: 42 }} onClick={onReload}>
            Sign in
          </button>
        </section>
      )}
      <style>{'@keyframes bb-restore-slide{0%{transform:translateX(-100%)}100%{transform:translateX(250%)}}'}</style>
    </div>
  );
}

const S: Record<string, React.CSSProperties> = {
  viewport: {
    position: 'fixed', inset: 0, zIndex: 3000, background: 'var(--bg)', color: 'var(--text)',
    display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 24,
  },
  card: {
    width: '100%', maxWidth: 440, padding: 28, borderRadius: 16, background: 'var(--surface)',
    border: '1px solid var(--border)', display: 'flex', flexDirection: 'column', gap: 14, alignItems: 'center', textAlign: 'center',
  },
  title: { margin: 0, fontSize: 22, fontWeight: 800 },
  text: { margin: 0, fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6 },
  track: { width: '100%', height: 6, borderRadius: 3, background: 'var(--border)', overflow: 'hidden' },
  bar: { width: '40%', height: '100%', background: 'var(--accent)', animation: 'bb-restore-slide 1.4s ease-in-out infinite' },
};
