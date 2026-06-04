/**
 * Defines mobile Mobile Settings View behavior for the BoogieBox React client.
 */

import React, { useEffect, useState } from 'react';
import { api, getStreamDirect, setStreamDirect } from '../../api';
import { APP_VERSION } from '../../version';
import type { AuthUser } from '../../types';
import MobileBottomSheet from '../components/MobileBottomSheet';

type SettingsState = Record<string, string>;

/** Mobile Settings View is part of this module's public API. */
export default function MobileSettingsView({
  currentUser,
  onClose,
}: {
  currentUser: AuthUser;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<SettingsState>({});
  const [streamDirect, setStreamDirectState] = useState(getStreamDirect());
  const [dlna, setDlna] = useState<{ running: boolean; port: number | null; friendlyName: string | null } | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    api.settings.get().then(setSettings).catch((err: Error) => setError(err.message || 'Could not load settings.'));
    api.dlna.status().then(setDlna).catch(() => {});
  }, []);

  const updateSetting = async (key: string, value: string) => {
    setSettings((current) => ({ ...current, [key]: value }));
    await api.settings.update({ [key]: value });
  };

  return (
    <MobileBottomSheet title="Settings" onClose={onClose}>
      <div style={styles.content}>
        {error ? <div style={styles.error}>{error}</div> : null}
        <section style={styles.section}>
          <h3 style={styles.heading}>Playback</h3>
          <label style={styles.row}>
            <span>Stream Direct</span>
            <input
              type="checkbox"
              checked={streamDirect}
              onChange={(event) => {
                setStreamDirect(event.target.checked);
                setStreamDirectState(event.target.checked);
              }}
            />
          </label>
          <label style={styles.row}>
            <span>Replay Gain</span>
            <input
              type="checkbox"
              checked={settings.replayGainEnabled === 'true'}
              onChange={(event) => updateSetting('replayGainEnabled', String(event.target.checked)).catch(() => {})}
            />
          </label>
          <label style={styles.row}>
            <span>Crossfade</span>
            <input
              type="checkbox"
              checked={settings.crossfadeMode === 'auto'}
              onChange={(event) => updateSetting('crossfadeMode', event.target.checked ? 'auto' : 'off').catch(() => {})}
            />
          </label>
        </section>
        <section style={styles.section}>
          <h3 style={styles.heading}>Quality</h3>
          <label style={styles.selectRow}>
            <span>Transcode quality</span>
            <select
              value={settings.transcodeQuality || 'low'}
              onChange={(event) => updateSetting('transcodeQuality', event.target.value).catch(() => {})}
              style={styles.select}
            >
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
              <option value="lossless">Lossless</option>
            </select>
          </label>
        </section>
        <section style={styles.section}>
          <h3 style={styles.heading}>Account</h3>
          <div style={styles.muted}>{currentUser.username}</div>
          <button type="button" style={styles.button} onClick={() => api.auth.logout().then(() => window.location.reload())}>Logout</button>
        </section>
        <section style={styles.section}>
          <h3 style={styles.heading}>DLNA</h3>
          <div style={styles.muted}>{dlna ? `${dlna.running ? 'Running' : 'Stopped'}${dlna.port ? ` on ${dlna.port}` : ''}` : 'Status unavailable'}</div>
        </section>
        <section style={styles.section}>
          <h3 style={styles.heading}>App Info</h3>
          <div style={styles.muted}>Version {APP_VERSION}</div>
        </section>
      </div>
    </MobileBottomSheet>
  );
}

const styles: Record<string, React.CSSProperties> = {
  content: { display: 'grid', gap: 16 },
  section: { display: 'grid', gap: 10, padding: '12px 0', borderTop: '1px solid var(--border)' },
  heading: { margin: 0, color: 'var(--text)', fontSize: 13, fontWeight: 800, textTransform: 'uppercase', letterSpacing: 0.8 },
  row: { minHeight: 48, display: 'flex', alignItems: 'center', justifyContent: 'space-between', color: 'var(--text)', fontSize: 15 },
  selectRow: { display: 'grid', gap: 8, color: 'var(--text)', fontSize: 15 },
  select: { minHeight: 44, borderRadius: 12, border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', fontFamily: 'inherit', padding: '0 12px' },
  button: { minHeight: 46, borderRadius: 14, border: '1px solid var(--border)', background: 'var(--bg)', color: 'var(--text)', fontFamily: 'inherit', fontWeight: 700 },
  muted: { color: 'var(--text-muted)', fontSize: 14 },
  error: { padding: 12, borderRadius: 12, border: '1px solid var(--border)', color: 'var(--text)', background: 'color-mix(in srgb, var(--accent) 12%, var(--surface))' },
};
