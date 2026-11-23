/**
 * Mobile Settings → Scrobbling: connect the signed-in user's Libre.fm and ListenBrainz accounts and
 * their own ListenBrainz-compatible server. Mirrors the desktop panel (components/ScrobblingPanel.tsx) and
 * shares its logic.
 */

import React, { useState } from 'react';
import { formatWhen } from '../../components/ConnectedAppsPanel';
import {
  accountLabel,
  disconnectTitle,
  ScrobbleIcon,
  SERVER_COPY,
  SERVICE_NAMES,
  useScrobbleAccounts,
  type SignInService,
} from '../../components/ScrobblingPanel';
import { hybridMobileContentStyles } from '../../hybridPreview';
import type { ScrobbleService } from '../../scrobbling';
import MobileConfirmationSheet from './MobileConfirmationSheet';

/** Mobile Scrobbling is part of this module's public API. */
export default function MobileScrobbling() {
  const s = useScrobbleAccounts();
  const [token, setToken] = useState('');
  const [serverAddress, setServerAddress] = useState('');
  const [serverToken, setServerToken] = useState('');
  const [confirm, setConfirm] = useState<ScrobbleService | null>(null);
  if (!s.loaded) return null;
  const listenbrainz = s.account('listenbrainz');
  const server = s.account('server');
  const failed = s.result?.startsWith('Error: ');
  const message = s.result ? s.result.replace(/^Error: /, '') : '';
  const serverReady = !s.busy && serverAddress.trim() !== '' && serverToken.trim() !== '';

  const row = (service: ScrobbleService) => {
    const account = s.account(service);
    return (
      <div style={styles.row}>
        <span style={styles.icon}>{service === 'server' ? <ScrobbleIcon.Server /> : <ScrobbleIcon.Service />}</span>
        <span style={styles.label}>{SERVICE_NAMES[service]}</span>
        <strong style={styles.value}>
          {account ? `${accountLabel(account)} · ${formatWhen(account.lastScrobbleAt) ?? 'No scrobbles yet'}` : 'Not connected'}
        </strong>
      </div>
    );
  };

  const signIn = (service: SignInService) => {
    const name = SERVICE_NAMES[service];
    const account = s.account(service);
    const waiting = s.waitingFor !== null;
    return (
      <>
        {row(service)}
        {account?.needsReconnect ? <div role="alert" style={styles.error}>{name} stopped accepting this account. Reconnect to keep scrobbling.</div> : null}
        {!account || account.needsReconnect ? (
          <button type="button" disabled={s.busy || waiting} style={{ ...styles.primary, ...(s.busy || waiting ? styles.disabled : null) }} onClick={() => void s.connectSignIn(service)}>
            {account ? `Reconnect ${name}` : `Connect ${name}`}
          </button>
        ) : null}
        {s.waitingFor === service ? <div role="status" style={styles.hint}>Finish signing in to {name} in the window that opened.</div> : null}
        {account ? (
          <button type="button" disabled={s.busy} style={styles.danger} onClick={() => setConfirm(service)}>Disconnect {name}</button>
        ) : null}
      </>
    );
  };

  return (
    <section aria-labelledby="mobile-settings-scrobbling" style={styles.section}>
      <div>
        <div style={styles.eyebrow}>Listening history</div>
        <h3 id="mobile-settings-scrobbling" style={styles.heading}>Scrobbling</h3>
      </div>
      <p style={styles.description}>
        Send what you play to your own Libre.fm or ListenBrainz account, or your own scrobble server.
      </p>

      {signIn('librefm')}

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

      {row('server')}
      {server?.needsReconnect ? <div role="alert" style={styles.error}>{SERVER_COPY.reconnect}</div> : null}
      {!server || server.needsReconnect ? (
        <>
          <div style={styles.hint}>{SERVER_COPY.hint}</div>
          <label style={styles.field}>
            {SERVER_COPY.addressLabel}
            <input
              type="url"
              inputMode="url"
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              maxLength={2048}
              placeholder={SERVER_COPY.addressPlaceholder}
              value={serverAddress}
              style={styles.input}
              onChange={(e) => setServerAddress(e.target.value)}
            />
          </label>
          <label style={styles.field}>
            {SERVER_COPY.tokenLabel}
            <input type="password" autoComplete="off" value={serverToken} maxLength={256} style={styles.input} onChange={(e) => setServerToken(e.target.value)} />
          </label>
          <button
            type="button"
            disabled={!serverReady}
            style={{ ...styles.primary, ...(!serverReady ? styles.disabled : null) }}
            onClick={() => {
              void s.connectServer(serverAddress, serverToken).then((ok) => { if (ok) { setServerAddress(''); setServerToken(''); } });
            }}
          >
            {SERVER_COPY.connect}
          </button>
        </>
      ) : null}
      {server ? (
        <button type="button" disabled={s.busy} style={styles.danger} onClick={() => setConfirm('server')}>Disconnect your server</button>
      ) : null}

      {message ? <div role={failed ? 'alert' : 'status'} style={failed ? styles.error : styles.saved}>{message}</div> : null}

      <MobileConfirmationSheet
        open={confirm !== null}
        title={confirm ? disconnectTitle(confirm) : ''}
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
  input: { minHeight: 44, minWidth: 0, padding: '0 12px', border: '1px solid var(--divider-subtle)', borderRadius: 11, background: 'var(--surface-subtle)', color: 'var(--text)', fontFamily: 'inherit', fontSize: 15 },
  hint: { color: 'var(--text-muted)', fontSize: 12 },
  saved: { ...hybridMobileContentStyles.feedback, borderColor: 'color-mix(in srgb, var(--success) 32%, var(--divider-subtle))', background: 'color-mix(in srgb, var(--success) 9%, var(--surface))', color: 'var(--success)' },
  error: { ...hybridMobileContentStyles.feedback, ...hybridMobileContentStyles.feedbackError },
  primary: { minHeight: 44, border: 'none', borderRadius: 11, background: 'var(--accent)', color: 'var(--on-accent)', fontFamily: 'inherit', fontSize: 14, fontWeight: 750 },
  danger: { minHeight: 44, border: '1px solid color-mix(in srgb, var(--danger) 28%, var(--divider-subtle))', borderRadius: 11, background: 'color-mix(in srgb, var(--danger) 8%, var(--surface))', color: 'var(--danger)', fontFamily: 'inherit', fontSize: 13, fontWeight: 750 },
  disabled: { opacity: 0.55, cursor: 'not-allowed' },
};
