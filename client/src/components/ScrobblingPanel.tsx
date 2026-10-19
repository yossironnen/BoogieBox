/**
 * Settings → User Settings → Scrobbling: connect the signed-in user's own Last.fm account. ListenBrainz is
 * connected from Settings → Integrations (`ListenBrainzConnector` in SettingsPage). Plays from the web player,
 * Play On speakers and Subsonic apps are then sent by the server.
 * `useScrobbleAccounts` holds the logic and is shared with the Integrations card and the mobile panel.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles, hybridSettingsStyles } from '../hybridPreview';
import { plausibleListenBrainzToken, type ScrobbleAccount, type ScrobbleService } from '../scrobbling';
import ConfirmModal from './ConfirmModal';
import { formatWhen, SubsonicIcon } from './ConnectedAppsPanel';
import { InlineStatus } from './settingsControls';

/** How long the panel waits for the Last.fm sign-in to finish. */
const LASTFM_WAIT_MS = 5 * 60 * 1000;
const LASTFM_POLL_MS = 3000;

export const SERVICE_NAMES: Record<ScrobbleService, string> = {
  lastfm: 'Last.fm',
  listenbrainz: 'ListenBrainz',
};

const svg = (size: number, children: React.ReactNode) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {children}
  </svg>
);

/** Icons for scrobbling rows. */
export const ScrobbleIcon = {
  /** Broadcast arcs: a service account. */
  Service: ({ size = 16 }: { size?: number }) => svg(size, <><circle cx="12" cy="12" r="2" /><path d="M16.2 7.8a6 6 0 0 1 0 8.4M7.8 16.2a6 6 0 0 1 0-8.4" /></>),
  /** Up arrow into a tray: plays waiting to send. */
  Pending: ({ size = 16 }: { size?: number }) => svg(size, <><path d="M12 15V3M7 8l5-5 5 5" /><path d="M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4" /></>),
  Warning: ({ size = 16 }: { size?: number }) => svg(size, <><path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /><path d="M12 9v4M12 17h.01" /></>),
};

/** Scrobbling accounts state and actions (desktop and mobile panels). */
export function useScrobbleAccounts() {
  const [accounts, setAccounts] = useState<ScrobbleAccount[]>([]);
  const [lastfmAvailable, setLastfmAvailable] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [waitingLastFm, setWaitingLastFm] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const deadlineRef = useRef(0);

  const load = useCallback(async () => {
    try {
      const res = await api.scrobble.accounts();
      setAccounts(res.accounts);
      setLastfmAvailable(res.lastfmAvailable);
      return res.accounts;
    } catch {
      return null;
    } finally {
      setLoaded(true);
    }
  }, []);

  const stopWaiting = useCallback(() => {
    if (pollRef.current) clearInterval(pollRef.current);
    pollRef.current = null;
    setWaitingLastFm(false);
  }, []);

  useEffect(() => {
    void load();
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, [load]);

  const account = (service: ScrobbleService) => accounts.find((a) => a.service === service) ?? null;

  const connectListenBrainz = async (token: string): Promise<boolean> => {
    if (!plausibleListenBrainzToken(token)) {
      setResult('Error: That does not look like a ListenBrainz user token');
      return false;
    }
    setBusy(true);
    setResult(null);
    try {
      const res = await api.scrobble.connectListenBrainz(token.trim());
      setResult(res.accountName ? `Connected to ListenBrainz as ${res.accountName}` : 'Connected to ListenBrainz');
      await load();
      return true;
    } catch (error) {
      setResult(`Error: ${error instanceof Error ? error.message : 'Could not connect'}`);
      return false;
    } finally {
      setBusy(false);
    }
  };

  /** Opens the Last.fm sign-in page and waits for the account to appear. */
  const connectLastFm = async () => {
    setBusy(true);
    setResult(null);
    try {
      const { url } = await api.scrobble.connectLastFm(window.location.origin);
      window.open(url, '_blank', 'noopener,noreferrer');
      setWaitingLastFm(true);
      deadlineRef.current = Date.now() + LASTFM_WAIT_MS;
      const before = account('lastfm')?.connectedAt ?? null;
      if (pollRef.current) clearInterval(pollRef.current);
      pollRef.current = setInterval(() => {
        void load().then((list) => {
          const now = list?.find((a) => a.service === 'lastfm');
          if (now && !now.needsReconnect && now.connectedAt !== before) {
            stopWaiting();
            setResult(now.accountName ? `Connected to Last.fm as ${now.accountName}` : 'Connected to Last.fm');
          } else if (Date.now() > deadlineRef.current) {
            stopWaiting();
          }
        });
      }, LASTFM_POLL_MS);
    } catch (error) {
      setResult(`Error: ${error instanceof Error ? error.message : 'Could not start the Last.fm sign-in'}`);
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async (service: ScrobbleService) => {
    setBusy(true);
    setResult(null);
    try {
      await api.scrobble.disconnect(service);
      setResult(`Disconnected from ${SERVICE_NAMES[service]}`);
      await load();
    } catch (error) {
      setResult(`Error: ${error instanceof Error ? error.message : 'Could not disconnect'}`);
    } finally {
      setBusy(false);
    }
  };

  return {
    loaded,
    accounts,
    account,
    lastfmAvailable,
    busy,
    waitingLastFm,
    result,
    connectListenBrainz,
    connectLastFm,
    cancelLastFmWait: stopWaiting,
    disconnect,
  };
}

/** One service's status row. */
export function AccountRow({ service, account }: { service: ScrobbleService; account: ScrobbleAccount | null }) {
  const last = formatWhen(account?.lastScrobbleAt);
  return (
    <div style={S.stats}>
      <div style={S.stat}>
        <span style={S.statIcon}><ScrobbleIcon.Service /></span>
        <span style={S.statLabel}>{SERVICE_NAMES[service]}</span>
        <strong style={S.statValue}>{account ? (account.accountName || 'Connected') : 'Not connected'}</strong>
      </div>
      {account && (
        <>
          <div style={S.stat}>
            <span style={S.statIcon}><SubsonicIcon.Clock /></span>
            <span style={S.statLabel}>Last scrobble</span>
            <strong style={S.statValue}>{last ?? 'Never'}</strong>
          </div>
          <div style={S.stat}>
            <span style={S.statIcon}><ScrobbleIcon.Pending /></span>
            <span style={S.statLabel}>Waiting to send</span>
            <strong style={S.statValue}>{account.pending.toLocaleString()}</strong>
          </div>
        </>
      )}
    </div>
  );
}

/** Scrobbling panel for the User Settings tab. */
export default function ScrobblingPanel() {
  const s = useScrobbleAccounts();
  const [confirm, setConfirm] = useState<ScrobbleService | null>(null);
  const lastfm = s.account('lastfm');

  if (!s.loaded) return null;

  return (
    <section aria-label="Scrobbling" style={hybridSettingsStyles.panel}>
      <div style={hybridSettingsStyles.panelHeader}>
        <div>
          <div style={hybridSettingsStyles.sectionTitle}>Scrobbling</div>
          <div style={hybridSettingsStyles.panelDescription}>
            Send what you play here, on Play On speakers and in Subsonic apps to your own Last.fm account. ListenBrainz is under Integrations.
          </div>
        </div>
      </div>

      <div style={S.service}>
        <AccountRow service="lastfm" account={lastfm} />
        {lastfm?.needsReconnect && (
          <div role="alert" style={S.warning}><ScrobbleIcon.Warning size={14} /> Last.fm stopped accepting this account. Reconnect to keep scrobbling.</div>
        )}
        <div style={S.actions}>
          {(!lastfm || lastfm.needsReconnect) && (
            s.lastfmAvailable ? (
              <button
                type="button"
                disabled={s.busy || s.waitingLastFm}
                onClick={() => void s.connectLastFm()}
                style={{ ...hybridControlStyles.primaryButton, ...(s.busy || s.waitingLastFm ? hybridControlStyles.disabled : {}) }}
              >
                {lastfm ? 'Reconnect Last.fm' : 'Connect Last.fm'}
              </button>
            ) : (
              <span style={S.hint}>An admin needs to add the Last.fm API key and shared secret in Settings → Providers first.</span>
            )
          )}
          {lastfm && (
            <button type="button" disabled={s.busy} onClick={() => setConfirm('lastfm')} style={hybridControlStyles.dangerButton}>
              Disconnect
            </button>
          )}
        </div>
        {s.waitingLastFm && (
          <div style={S.hint}>
            Finish signing in to Last.fm in the window that opened.{' '}
            <button type="button" style={S.linkButton} onClick={s.cancelLastFmWait}>Cancel</button>
          </div>
        )}
      </div>

      <InlineStatus busy={s.busy} busyText="Working…" result={s.result} />

      {confirm && (
        <ConfirmModal
          title={`Disconnect ${SERVICE_NAMES[confirm]}?`}
          message="Plays waiting to be sent to it are dropped."
          confirmLabel="Disconnect"
          tone="danger"
          onConfirm={() => { const service = confirm; setConfirm(null); void s.disconnect(service); }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </section>
  );
}

const S: Record<string, React.CSSProperties> = {
  service: { display: 'grid', gap: 10, padding: '12px 0', borderTop: '1px solid var(--divider-subtle)' },
  stats: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 10 },
  stat: {
    display: 'grid',
    gridTemplateColumns: '20px minmax(0, 1fr)',
    gridTemplateRows: 'auto auto',
    columnGap: 8,
    rowGap: 2,
    padding: '10px 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 10,
    background: 'var(--surface-subtle)',
  },
  statIcon: { gridRow: '1 / span 2', display: 'inline-flex', alignItems: 'center', color: 'var(--accent)', opacity: 0.85 },
  statLabel: { fontSize: 12, color: 'var(--text-muted)' },
  statValue: { fontSize: 14, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  warning: { display: 'flex', alignItems: 'center', gap: 6, fontSize: 13, color: 'var(--warning, #f59e0b)' },
  actions: { display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' },
  hint: { fontSize: 13, color: 'var(--text-muted)' },
  linkButton: { border: 'none', background: 'none', padding: 0, color: 'var(--accent)', cursor: 'pointer', fontFamily: 'inherit', fontSize: 13 },
};
