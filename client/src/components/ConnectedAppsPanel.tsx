/**
 * Settings → User Settings → Connected apps: the signed-in user's Subsonic password for third-party apps
 * (Symfonium, Feishin, Supersonic, Amperfy …). Hidden until an admin turns the Subsonic API on.
 * Shared pieces (icons, copy rows, password fields) are reused by Users and the Advanced Subsonic card.
 * See wip/opensubsonic-api-implementation-plan.md §9.2.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles, hybridSettingsStyles } from '../hybridPreview';
import { subsonicPasswordChecks, subsonicPasswordProblem, type SubsonicCredentials } from '../subsonic';
import { parseServerDate } from '../utils';
import ConfirmModal from './ConfirmModal';
import { InlineStatus } from './settingsControls';

const svg = (size: number, children: React.ReactNode) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {children}
  </svg>
);

/** Icons for Subsonic surfaces (inline, stroke-based). */
export const SubsonicIcon = {
  Key: ({ size = 16 }: { size?: number }) => svg(size, <><circle cx="8" cy="15" r="4" /><path d="M10.8 12.2L20 3M17 6l3 3M15 8l2 2" /></>),
  Clock: ({ size = 16 }: { size?: number }) => svg(size, <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></>),
  Plug: ({ size = 16 }: { size?: number }) => svg(size, <><path d="M9 2v6M15 2v6" /><path d="M6 8h12v4a6 6 0 0 1-12 0z" /><path d="M12 18v4" /></>),
  Server: ({ size = 16 }: { size?: number }) => svg(size, <><rect x="3" y="4" width="18" height="7" rx="2" /><rect x="3" y="13" width="18" height="7" rx="2" /><path d="M7 7.5h.01M7 16.5h.01" /></>),
  User: ({ size = 16 }: { size?: number }) => svg(size, <><circle cx="12" cy="8" r="4" /><path d="M4 21a8 8 0 0 1 16 0" /></>),
  Copy: ({ size = 15 }: { size?: number }) => svg(size, <><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></>),
  Check: ({ size = 14 }: { size?: number }) => svg(size, <path d="M5 12l5 5L20 7" />),
  Circle: ({ size = 14 }: { size?: number }) => svg(size, <circle cx="12" cy="12" r="8" />),
};

/** Local date/time for a server timestamp, or null. */
export function formatWhen(value: string | null | undefined): string | null {
  const date = parseServerDate(value ?? null);
  return date && !Number.isNaN(date.getTime()) ? date.toLocaleString() : null;
}

/** A labelled value with an icon-only copy button. */
export function CopyValue({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    const clipboard = typeof navigator !== 'undefined' ? navigator.clipboard : undefined;
    if (!clipboard) return;
    clipboard.writeText(value).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  };
  return (
    <div style={S.copyRow}>
      <span style={S.statIcon}>{icon}</span>
      <span style={S.copyLabel}>{label}</span>
      <code style={S.copyValue}>{value}</code>
      <button type="button" style={hybridControlStyles.iconButton} title={copied ? 'Copied' : `Copy ${label.toLowerCase()}`} aria-label={`Copy ${label.toLowerCase()}`} onClick={copy}>
        {copied ? <SubsonicIcon.Check /> : <SubsonicIcon.Copy />}
      </button>
    </div>
  );
}

/** New password + confirmation with live rule feedback. */
export function PasswordFields({
  password,
  confirmation,
  onPassword,
  onConfirmation,
  label = 'New Subsonic password',
}: {
  password: string;
  confirmation: string;
  onPassword: (value: string) => void;
  onConfirmation: (value: string) => void;
  label?: string;
}) {
  const checks = subsonicPasswordChecks(password);
  const rules: Array<[boolean, string]> = [
    [checks.length, 'At least 8 characters'],
    [checks.symbol, 'At least one symbol'],
    [password.length > 0 && password === confirmation, 'Both entries match'],
  ];
  return (
    <div style={{ display: 'grid', gap: 10 }}>
      <div style={S.fieldGrid}>
        <label style={S.fieldLabel}>
          {label}
          <input type="password" autoComplete="new-password" value={password} maxLength={256} onChange={(e) => onPassword(e.target.value)} style={S.input} />
        </label>
        <label style={S.fieldLabel}>
          Confirm password
          <input type="password" autoComplete="new-password" value={confirmation} maxLength={256} onChange={(e) => onConfirmation(e.target.value)} style={S.input} />
        </label>
      </div>
      <ul aria-label="Password rules" style={S.rules}>
        {rules.map(([ok, text]) => (
          <li key={text} style={{ ...S.rule, color: ok ? 'var(--success)' : 'var(--text-muted)' }}>
            {ok ? <SubsonicIcon.Check /> : <SubsonicIcon.Circle />}
            {text}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Connected apps panel for the User Settings tab. */
export default function ConnectedAppsPanel({ username }: { username: string }) {
  const [creds, setCreds] = useState<SubsonicCredentials | null>(null);
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);

  const load = useCallback(() => {
    api.subsonic.credentials().then(setCreds).catch(() => setCreds(null));
  }, []);
  useEffect(load, [load]);

  if (!creds?.enabled) return null;

  const problem = subsonicPasswordProblem(password, confirmation);
  const save = async () => {
    if (problem) { setResult(`Error: ${problem}`); return; }
    setSaving(true);
    setResult(null);
    try {
      await api.subsonic.setPassword(password);
      setPassword('');
      setConfirmation('');
      setResult('Subsonic password saved');
      load();
    } catch (error) {
      setResult(`Error: ${error instanceof Error ? error.message : 'Could not save'}`);
    } finally {
      setSaving(false);
    }
  };
  const remove = async () => {
    setSaving(true);
    setResult(null);
    try {
      await api.subsonic.clearPassword();
      setResult('Subsonic password removed');
      load();
    } catch (error) {
      setResult(`Error: ${error instanceof Error ? error.message : 'Could not remove'}`);
    } finally {
      setSaving(false);
    }
  };

  const lastUsed = formatWhen(creds.lastUsedAt);
  return (
    <section aria-label="Connected apps" style={hybridSettingsStyles.panel}>
      <div style={hybridSettingsStyles.panelHeader}>
        <div>
          <div style={hybridSettingsStyles.sectionTitle}>Connected apps</div>
          <div style={hybridSettingsStyles.panelDescription}>
            Use Subsonic-compatible apps such as Symfonium, Feishin, Supersonic or Amperfy with your library.
            Sign in to them with the server address, your username and a Subsonic password.
          </div>
        </div>
      </div>

      <div style={S.stats}>
        <div style={S.stat}>
          <span style={S.statIcon}><SubsonicIcon.Key /></span>
          <span style={S.statLabel}>Subsonic password</span>
          <strong style={S.statValue}>{creds.hasPassword ? 'Set' : 'Not set'}</strong>
        </div>
        <div style={S.stat}>
          <span style={S.statIcon}><SubsonicIcon.Clock /></span>
          <span style={S.statLabel}>Last used</span>
          <strong style={S.statValue}>{lastUsed ?? 'Never'}</strong>
        </div>
        <div style={S.stat}>
          <span style={S.statIcon}><SubsonicIcon.Plug /></span>
          <span style={S.statLabel}>Last app</span>
          <strong style={S.statValue}>{creds.lastClient ?? '—'}</strong>
        </div>
      </div>

      <div style={S.copyList}>
        {creds.serverUrls.map((url) => <CopyValue key={url} icon={<SubsonicIcon.Server />} label="Server address" value={url} />)}
        <CopyValue icon={<SubsonicIcon.User />} label="Username" value={username} />
      </div>

      <PasswordFields
        password={password}
        confirmation={confirmation}
        onPassword={(v) => { setPassword(v); setResult(null); }}
        onConfirmation={(v) => { setConfirmation(v); setResult(null); }}
      />
      <div style={S.hint}>Use a password you don't use anywhere else. Your PIN is not used by apps.</div>
      <div style={S.actions}>
        <button
          type="button"
          disabled={saving || problem !== null}
          onClick={() => void save()}
          style={{ ...hybridControlStyles.primaryButton, ...(saving || problem ? hybridControlStyles.disabled : {}) }}
        >
          {creds.hasPassword ? 'Change password' : 'Set password'}
        </button>
        {creds.hasPassword && (
          <button type="button" disabled={saving} onClick={() => setConfirmRemove(true)} style={hybridControlStyles.dangerButton}>
            Remove password
          </button>
        )}
      </div>
      <InlineStatus busy={saving} busyText="Saving…" result={result} />

      {confirmRemove && (
        <ConfirmModal
          title="Remove your Subsonic password?"
          message="Apps signed in with it stop working until you set a new one."
          confirmLabel="Remove"
          tone="danger"
          onConfirm={() => { setConfirmRemove(false); void remove(); }}
          onCancel={() => setConfirmRemove(false)}
        />
      )}
    </section>
  );
}

const S: Record<string, React.CSSProperties> = {
  stats: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 10, margin: '14px 0' },
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
  copyList: { display: 'grid', gap: 6, marginBottom: 16 },
  copyRow: { display: 'grid', gridTemplateColumns: '20px 130px minmax(0, 1fr) auto', alignItems: 'center', gap: 8 },
  copyLabel: { fontSize: 13, color: 'var(--text-muted)' },
  copyValue: { fontSize: 13, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
  fieldGrid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12 },
  fieldLabel: { display: 'flex', flexDirection: 'column', gap: 4, fontSize: 13, color: 'var(--text-muted)' },
  input: { ...hybridControlStyles.field, minHeight: 38, padding: '7px 10px', fontSize: 14 },
  rules: { display: 'flex', flexWrap: 'wrap', gap: 14, margin: 0, padding: 0, listStyle: 'none' },
  rule: { display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 13 },
  hint: { fontSize: 13, color: 'var(--text-muted)', margin: '10px 0' },
  actions: { display: 'flex', gap: 8, flexWrap: 'wrap' },
};
