/**
 * Mobile Settings → Scrobbling: connect the signed-in user's Last.fm and ListenBrainz accounts.
 * Mirrors the desktop panel (components/ScrobblingPanel.tsx) and shares its logic.
 */

import React, { useState } from 'react';
import { formatWhen } from '../../components/ConnectedAppsPanel';
import { ScrobbleIcon, SERVICE_NAMES, useScrobbleAccounts } from '../../components/ScrobblingPanel';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { ScrobbleService } from '../../scrobbling';
import MobileConfirmationSheet from './MobileConfirmationSheet';

/** Mobile Scrobbling is part of this module's public API. */
export default function MobileScrobbling() {
  const s = useScrobbleAccounts();
  const [token, setToken] = useState('');
  const [confirm, setConfirm] = useState<ScrobbleService | null>(null);
  if (!s.loaded) return null;
  const lastfm = s.account('lastfm');
  const listenbrainz = s.account('listenbrainz');
  const failed = s.result?.startsWith('Error: ');
  const message = s.result ? s.result.replace(/^Error: /, '') : '';

  const row = (service: ScrobbleService) => {
    const account = s.account(service);
    return (
      <div style={styles.row}>
        <span style={styles.icon}><ScrobbleIcon.Service /></span>
        <span style={styles.label}>{SERVICE_NAMES[service]}</span>
        <strong style={styles.value}>
          {account ? `${account.accountName || 'Connected'} · ${formatWhen(account.lastScrobbleAt) ?? 'No scrobbles yet'}` : 'Not connected'}
        </strong>
      </div>
    );
  };

  return (
    <section aria-labelledby="mobile-settings-scrobbling" style={styles.section}>
      <div>
        <div style={styles.eyebrow}>Listening history</div>
        <h3 id="mobile-settings-scrobbling" style={styles.heading}>Scrobbling</h3>
      </div>
      <p style={styles.description}>
        Send what you play to your own Last.fm or ListenBrainz account.
      </p>

      {row('lastfm')}
      {lastfm?.needsReconnect ? <div role="alert" style={styles.error}>Last.fm stopped accepting this account. Reconnect to keep scrobbling.</div> : null}
      {!lastfm || lastfm.needsReconnect ? (
        s.lastfmAvailable ? (
          <button type="button" disabled={s.busy || s.waitingLastFm} style={{ ...styles.primary, ...(s.busy || s.waitingLastFm ? styles.disabled : null) }} onClick={() => void s.connectLastFm()}>
            {lastfm ? 'Reconnect Last.fm' : 'Connect Last.fm'}
          </button>
        ) : (
          <div style={styles.hint}>An admin needs to add the Last.fm API key and shared secret first.</div>
        )
      ) : null}
      {s.waitingLastFm ? <div role="status" style={styles.hint}>Finish signing in to Last.fm in the window that opened.</div> : null}
      {lastfm ? (
        <button type="button" disabled={s.busy} style={styles.danger} onClick={() => setConfirm('lastfm')}>Disconnect Last.fm</button>
      ) : null}

      {row('listenbrainz')}
      {listenbrainz?.needsReconnect ? <div role="alert" style={styles.error}>ListenBrainz stopped accepting this token. Enter a new one.</div> : null}
      {!listenbrainz || listenbrainz.needsReconnect ? (
        <>
          <label style={styles.field}>
            ListenBrainz user token
            <input type="password" autoComplete="off" value={token} maxLength={128} style={styles.input} onChange={(e) => setToken(e.target.value)} />
          </label>
          <button
            type="button"
            disabled={s.busy || !token.trim()}
            style={{ ...styles.primary, ...(s.busy || !token.trim() ? styles.disabled : null) }}
            onClick={() => { void s.connectListenBrainz(token).then((ok) => { if (ok) setToken(''); }); }}
          >
            Connect ListenBrainz
          </button>
        </>
      ) : (
        <button type="button" disabled={s.busy} style={styles.danger} onClick={() => setConfirm('listenbrainz')}>Disconnect ListenBrainz</button>
      )}

      {message ? <div role={failed ? 'alert' : 'status'} style={failed ? styles.error : styles.saved}>{message}</div> : null}

      <MobileConfirmationSheet
        open={confirm !== null}
        title={confirm ? `Disconnect ${SERVICE_NAMES[confirm]}?` : ''}
        description="Plays waiting to be sent to it are dropped."
        confirmLabel="Disconnect"
        cancelLabel="Keep it"
        onClose={() => setConfirm(null)}
        onConfirm={async () => {
          const service = confirm;
          setConfirm(null);
          if (service) await s.disconnect(service);
        }}
      />
    </section>
  );
}

const styles: Record<string, React.CSSProperties> = {
  section: { display: 'grid', gap: 10, padding: 12, border: '1px solid var(--divider-subtle)', borderRadius: 16, background: 'var(--surface)' },
  eyebrow: hybridMobileContentStyles.eyebrow,
  heading: { margin: '3px 0 0', color: 'var(--text)', fontSize: 19, fontWeight: 800, letterSpacing: -0.25 },
  description: { margin: 0, color: 'var(--text-muted)', fontSize: 12, lineHeight: 1.5 },
  row: { minHeight: 40, display: 'grid', gridTemplateColumns: '20px auto minmax(0, 1fr)', alignItems: 'center', gap: 8, paddingTop: 6, borderTop: '1px solid var(--divider-subtle)' },
  icon: { display: 'inline-flex', color: 'var(--accent)', opacity: 0.85 },
  label: { color: 'var(--text-muted)', fontSize: 12 },
  value: { color: 'var(--text)', fontSize: 13, textAlign: 'right', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  field: { display: 'grid', gap: 5, color: 'var(--text-muted)', fontSize: 12, fontWeight: 650 },
  input: { minHeight: 44, padding: '0 12px', border: '1px solid var(--divider-subtle)', borderRadius: 11, background: 'var(--surface-subtle)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 15 },
  hint: { color: 'var(--text-muted)', fontSize: 12 },
  saved: { ...hybridMobileContentStyles.feedback, borderColor: 'color-mix(in srgb, var(--success) 32%, var(--divider-subtle))', background: 'color-mix(in srgb, var(--success) 9%, var(--surface))', color: 'var(--success)' },
  error: { ...hybridMobileContentStyles.feedback, ...hybridMobileContentStyles.feedbackError },
  primary: { minHeight: 44, border: 'none', borderRadius: 11, background: 'var(--accent)', color: 'var(--on-accent)', fontFamily: 'inherit', fontSize: 14, fontWeight: 750 },
  danger: { minHeight: 44, border: '1px solid color-mix(in srgb, var(--danger) 28%, var(--divider-subtle))', borderRadius: 11, background: 'color-mix(in srgb, var(--danger) 8%, var(--surface))', color: 'var(--danger)', fontFamily: 'inherit', fontSize: 13, fontWeight: 750 },
  disabled: { opacity: 0.55, cursor: 'not-allowed' },
};
