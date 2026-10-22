/**
 * Mobile Settings → Connected apps: the signed-in user's Subsonic password for third-party apps.
 * Mirrors the desktop panel (components/ConnectedAppsPanel.tsx); hidden until the Subsonic API is on.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../../api';
import {
  API_KEY_HELP,
  apiKeyStatus,
  CopyValue,
  formatWhen,
  SubsonicIcon,
  useApiKey,
  usePasswordMask,
} from '../../components/ConnectedAppsPanel';
import { hybridMobileContentStyles } from '../../hybridPreview';
import { subsonicPasswordChecks, subsonicPasswordProblem, type SubsonicCredentials } from '../../subsonic';
import MobileConfirmationSheet from './MobileConfirmationSheet';

/** Mobile Connected Apps is part of this module's public API. */
export default function MobileConnectedApps({ username }: { username: string }) {
  const [creds, setCreds] = useState<SubsonicCredentials | null>(null);
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState('');
  const [error, setError] = useState('');
  const [confirmRemove, setConfirmRemove] = useState(false);

  const load = useCallback(() => {
    api.subsonic.credentials().then(setCreds).catch(() => setCreds(null));
  }, []);
  useEffect(load, [load]);
  const mask = usePasswordMask(creds);
  const apiKey = useApiKey(load);
  const [confirmKey, setConfirmKey] = useState<'replace' | 'revoke' | null>(null);

  if (!creds?.enabled) return null;

  const problem = subsonicPasswordProblem(password, confirmation);
  const checks = subsonicPasswordChecks(password);
  const save = async () => {
    if (problem) return;
    setBusy(true);
    setStatus('');
    setError('');
    try {
      await api.subsonic.setPassword(password);
      setPassword('');
      setConfirmation('');
      setStatus('Subsonic password saved.');
      load();
    } catch (saveError: any) {
      setError(saveError?.message || 'Could not save the password.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-labelledby="mobile-settings-apps" style={styles.section}>
      <div>
        <div style={styles.eyebrow}>Apps</div>
        <h3 id="mobile-settings-apps" style={styles.heading}>Connected apps</h3>
      </div>
      <p style={styles.description}>
        Sign in to Subsonic-compatible apps (Symfonium, Feishin, Amperfy…) with the server address, your username
        and a Subsonic password.
      </p>

      <div style={styles.row}>
        <span style={styles.icon}><SubsonicIcon.Key /></span>
        <span style={styles.label}>Subsonic password</span>
        <strong style={styles.value} aria-label={creds.hasPassword ? 'Set' : undefined}>{creds.hasPassword ? mask : 'Not set'}</strong>
      </div>
      <div style={styles.row}>
        <span style={styles.icon}><SubsonicIcon.Clock /></span>
        <span style={styles.label}>Last used</span>
        <strong style={styles.value}>
          {formatWhen(creds.lastUsedAt) ?? 'Never'}{creds.lastClient ? ` · ${creds.lastClient}` : ''}
        </strong>
      </div>
      {creds.serverUrls.map((url) => (
        <div key={url} style={styles.row}>
          <span style={styles.icon}><SubsonicIcon.Server /></span>
          <span style={styles.label}>Server address</span>
          <code style={styles.value}>{url}</code>
        </div>
      ))}
      <div style={styles.row}>
        <span style={styles.icon}><SubsonicIcon.User /></span>
        <span style={styles.label}>Username</span>
        <code style={styles.value}>{username}</code>
      </div>

      <label style={styles.field}>
        New Subsonic password
        <input type="password" autoComplete="new-password" value={password} maxLength={256} style={styles.input} onChange={(e) => { setPassword(e.target.value); setError(''); setStatus(''); }} />
      </label>
      <label style={styles.field}>
        Confirm password
        <input type="password" autoComplete="new-password" value={confirmation} maxLength={256} style={styles.input} onChange={(e) => { setConfirmation(e.target.value); setError(''); setStatus(''); }} />
      </label>
      <ul aria-label="Password rules" style={styles.rules}>
        {([
          [checks.length, 'At least 8 characters'],
          [checks.symbol, 'At least one symbol'],
          [password.length > 0 && password === confirmation, 'Both entries match'],
        ] as Array<[boolean, string]>).map(([ok, text]) => (
          <li key={text} style={{ ...styles.rule, color: ok ? 'var(--success)' : 'var(--text-muted)' }}>
            {ok ? <SubsonicIcon.Check /> : <SubsonicIcon.Circle />}
            {text}
          </li>
        ))}
      </ul>
      <div style={styles.hint}>Use a password you don't use anywhere else.</div>
      {status ? <div role="status" style={styles.saved}>{status}</div> : null}
      {error ? <div role="alert" style={styles.error}>{error}</div> : null}
      <button type="button" disabled={busy || problem !== null} style={{ ...styles.primary, ...(busy || problem ? styles.disabled : null) }} onClick={() => void save()}>
        {creds.hasPassword ? 'Change password' : 'Set password'}
      </button>
      {creds.hasPassword ? (
        <button type="button" disabled={busy} style={styles.danger} onClick={() => setConfirmRemove(true)}>
          Remove password
        </button>
      ) : null}

      <div style={styles.row}>
        <span style={styles.icon}><SubsonicIcon.Key /></span>
        <span style={styles.label}>API key</span>
        <strong style={styles.value}>{apiKeyStatus(creds)}</strong>
      </div>
      <div style={styles.hint}>{API_KEY_HELP}</div>
      {apiKey.shownKey ? (
        <div role="status" aria-label="Your API key" style={styles.keyBox}>
          <strong style={{ color: 'var(--text)' }}>Your API key</strong>
          <span>Copy it now. BoogieBox shows it only once.</span>
          <CopyValue icon={<SubsonicIcon.Key />} label="API key" value={apiKey.shownKey} />
          <button type="button" style={styles.primary} onClick={apiKey.dismiss}>Done</button>
        </div>
      ) : null}
      {apiKey.error ? <div role="alert" style={styles.error}>{apiKey.error}</div> : null}
      {creds.hasApiKey ? (
        <>
          <button type="button" disabled={apiKey.busy} style={styles.secondary} onClick={() => setConfirmKey('replace')}>
            Replace API key
          </button>
          <button type="button" disabled={apiKey.busy} style={styles.danger} onClick={() => setConfirmKey('revoke')}>
            Revoke API key
          </button>
        </>
      ) : (
        <button type="button" disabled={apiKey.busy} style={{ ...styles.primary, ...(apiKey.busy ? styles.disabled : null) }} onClick={() => void apiKey.create()}>
          Create API key
        </button>
      )}

      <MobileConfirmationSheet
        open={confirmKey !== null}
        title={confirmKey === 'replace' ? 'Replace your API key?' : 'Revoke your API key?'}
        description={confirmKey === 'replace' ? 'Apps using the current key are signed out.' : 'Apps using it are signed out.'}
        confirmLabel={confirmKey === 'replace' ? 'Replace API key' : 'Revoke API key'}
        cancelLabel="Cancel"
        onClose={() => setConfirmKey(null)}
        onConfirm={async () => {
          const action = confirmKey;
          setConfirmKey(null);
          await (action === 'replace' ? apiKey.create() : apiKey.revoke());
        }}
      />

      <MobileConfirmationSheet
        open={confirmRemove}
        title="Remove your Subsonic password?"
        description="Apps signed in with it stop working until you set a new one."
        confirmLabel="Remove"
        cancelLabel="Keep it"
        onClose={() => setConfirmRemove(false)}
        onConfirm={async () => {
          await api.subsonic.clearPassword();
          setConfirmRemove(false);
          setStatus('Subsonic password removed.');
          load();
        }}
      />
    </section>
  );
}

const styles: Record<string, React.CSSProperties> = {
  section: {
    display: 'grid',
    gap: 10,
    padding: 12,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'var(--surface)',
  },
  eyebrow: hybridMobileContentStyles.eyebrow,
  heading: { margin: '3px 0 0', color: 'var(--text)', fontSize: 19, fontWeight: 800, letterSpacing: -0.25 },
  description: { margin: 0, color: 'var(--text-muted)', fontSize: 12, lineHeight: 1.5 },
  row: {
    minHeight: 40,
    display: 'grid',
    gridTemplateColumns: '20px auto minmax(0, 1fr)',
    alignItems: 'center',
    gap: 8,
    paddingTop: 6,
    borderTop: '1px solid var(--divider-subtle)',
  },
  icon: { display: 'inline-flex', color: 'var(--accent)', opacity: 0.85 },
  label: { color: 'var(--text-muted)', fontSize: 12 },
  value: { color: 'var(--text)', fontSize: 13, textAlign: 'right', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  field: { display: 'grid', gap: 5, color: 'var(--text-muted)', fontSize: 12, fontWeight: 650 },
  input: {
    minHeight: 44,
    padding: '0 12px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 15,
  },
  rules: { display: 'grid', gap: 4, margin: 0, padding: 0, listStyle: 'none' },
  rule: { display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12 },
  hint: { color: 'var(--text-muted)', fontSize: 12 },
  saved: {
    ...hybridMobileContentStyles.feedback,
    borderColor: 'color-mix(in srgb, var(--success) 32%, var(--divider-subtle))',
    background: 'color-mix(in srgb, var(--success) 9%, var(--surface))',
    color: 'var(--success)',
  },
  error: { ...hybridMobileContentStyles.feedback, ...hybridMobileContentStyles.feedbackError },
  primary: {
    minHeight: 44,
    border: 'none',
    borderRadius: 11,
    background: 'var(--accent)',
    color: 'var(--on-accent)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 750,
  },
  danger: {
    minHeight: 44,
    border: '1px solid color-mix(in srgb, var(--danger) 28%, var(--divider-subtle))',
    borderRadius: 11,
    background: 'color-mix(in srgb, var(--danger) 8%, var(--surface))',
    color: 'var(--danger)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  secondary: {
    minHeight: 44,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
  keyBox: {
    display: 'grid',
    gap: 8,
    padding: 10,
    border: '1px solid color-mix(in srgb, var(--accent) 30%, var(--divider-subtle))',
    borderRadius: 12,
    background: 'var(--surface-subtle)',
    color: 'var(--text-muted)',
    fontSize: 12,
  },
  disabled: { opacity: 0.55 },
};
