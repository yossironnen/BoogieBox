/**
 * Defines mobile Mobile Settings View behavior for the BoogieBox React client.
 */

import React, { useEffect, useState } from 'react';
import { api, getStreamDirect, setStreamDirect } from '../../api';
import {
  HYBRID_THEME_MODES,
  hybridMobileContentStyles,
  type HybridThemeMode,
} from '../../hybridPreview';
import type { AppSettings, AuthUser } from '../../types';
import { DEFAULT_LOUDNESS_PREFS, LOUDNESS_MODES, type LoudnessPrefs } from '../../audio/loudness';
import {
  formatPreamp,
  LevellingIcon,
  LOUDNESS_MODE_HINTS,
  LOUDNESS_MODE_LABELS,
  LoudnessModeIcon,
  PREAMP_MAX_DB,
  PREAMP_MIN_DB,
  PREAMP_NOTE,
  PREAMP_STEP_DB,
} from '../../components/LoudnessSettings';
import { APP_VERSION } from '../../version';
import MobileBottomSheet from '../components/MobileBottomSheet';
import MobileAdminSettingsPanel from './MobileAdminSettingsPanel';
import MobileConnectedApps from '../components/MobileConnectedApps';
import MobileScrobbling from '../components/MobileScrobbling';

type SettingsState = Record<string, string>;
type ThemeColorKey =
  | 'colorBg'
  | 'colorSurface'
  | 'colorBorder'
  | 'colorAccent'
  | 'colorText'
  | 'colorTextMuted';

const THEME_COLOR_FIELDS: Array<{ key: ThemeColorKey; label: string }> = [
  { key: 'colorBg', label: 'Background' },
  { key: 'colorSurface', label: 'Surface' },
  { key: 'colorBorder', label: 'Border' },
  { key: 'colorAccent', label: 'Accent' },
  { key: 'colorText', label: 'Text' },
  { key: 'colorTextMuted', label: 'Muted text' },
];

function MobileSwitch({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div style={styles.settingRow}>
      <div style={styles.settingCopy}>
        <div style={styles.settingLabel}>{label}</div>
        <div style={styles.settingDescription}>{description}</div>
      </div>
      <button
        type="button"
        role="switch"
        aria-label={label}
        aria-checked={checked}
        style={styles.switchButton}
        onClick={() => onChange(!checked)}
      >
        <span style={{ ...styles.switchTrack, ...(checked ? styles.switchTrackActive : null) }}>
          <span style={{ ...styles.switchThumb, ...(checked ? styles.switchThumbActive : null) }} />
        </span>
      </button>
    </div>
  );
}

function MobileVolumeLevelling({ prefs, onChange }: {
  prefs: LoudnessPrefs;
  onChange: (change: Partial<LoudnessPrefs>) => void;
}) {
  const step = (direction: 1 | -1) => {
    const next = Math.round((prefs.preampDb + direction * PREAMP_STEP_DB) * 2) / 2;
    onChange({ preampDb: Math.min(PREAMP_MAX_DB, Math.max(PREAMP_MIN_DB, next)) });
  };
  return (
    <div role="group" aria-labelledby="mobile-volume-levelling" style={styles.levelling}>
      <div style={styles.levellingTitleRow}>
        <LevellingIcon size={18} />
        <div id="mobile-volume-levelling" style={styles.settingLabel}>Volume levelling</div>
      </div>
      <div style={styles.settingDescription}>Plays tracks at a similar volume without changing how they sound.</div>
      <div role="group" aria-label="Volume levelling mode" style={styles.levellingGrid}>
        {LOUDNESS_MODES.map((mode) => {
          const active = prefs.mode === mode;
          return (
            <button
              key={mode}
              type="button"
              aria-pressed={active}
              style={{ ...styles.levellingChip, ...(active ? styles.levellingChipActive : null) }}
              onClick={() => onChange({ mode })}
            >
              <LoudnessModeIcon mode={mode} />
              {LOUDNESS_MODE_LABELS[mode]}
            </button>
          );
        })}
      </div>
      <div style={styles.settingDescription}>{LOUDNESS_MODE_HINTS[prefs.mode]}</div>
      <div style={styles.preampRow}>
        <div style={styles.settingLabel}>Pre-amp</div>
        <div style={styles.stepper}>
          <button
            type="button"
            aria-label="Lower pre-amp"
            disabled={prefs.preampDb <= PREAMP_MIN_DB}
            style={styles.stepperButton}
            onClick={() => step(-1)}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M5 12h14" /></svg>
          </button>
          <output aria-live="polite" style={styles.stepperValue}>{formatPreamp(prefs.preampDb)}</output>
          <button
            type="button"
            aria-label="Raise pre-amp"
            disabled={prefs.preampDb >= PREAMP_MAX_DB}
            style={styles.stepperButton}
            onClick={() => step(1)}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
          </button>
        </div>
      </div>
      <div style={styles.settingDescription}>{PREAMP_NOTE}</div>
    </div>
  );
}

/** Mobile Settings View is part of this module's public API. */
export default function MobileSettingsView({
  currentUser,
  focus = null,
  onClose,
  appSettings,
  onAppSettingsChange,
  hybridThemeMode = 'dark',
  onHybridThemeModeChange,
  adaptiveAccentEnabled = true,
  onAdaptiveAccentEnabledChange,
  loudness = DEFAULT_LOUDNESS_PREFS,
  onLoudnessChange,
}: {
  currentUser: AuthUser;
  /** Open with the admin tools expanded at this section. */
  focus?: 'play-on' | null;
  onClose: () => void;
  appSettings?: AppSettings;
  onAppSettingsChange?: (settings: AppSettings) => void;
  hybridThemeMode?: HybridThemeMode;
  onHybridThemeModeChange?: (mode: HybridThemeMode) => void;
  adaptiveAccentEnabled?: boolean;
  onAdaptiveAccentEnabledChange?: (enabled: boolean) => void;
  loudness?: LoudnessPrefs;
  onLoudnessChange?: (change: Partial<LoudnessPrefs>) => void;
}) {
  const [settings, setSettings] = useState<SettingsState>({});
  const [streamDirect, setStreamDirectState] = useState(getStreamDirect());
  const [dlna, setDlna] = useState<{ running: boolean; port: number | null; friendlyName: string | null } | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [saveStatus, setSaveStatus] = useState('');

  useEffect(() => {
    let active = true;
    Promise.allSettled([api.settings.get(), api.dlna.status()])
      .then(([settingsResult, dlnaResult]) => {
        if (!active) return;
        if (settingsResult.status === 'fulfilled') {
          setSettings(settingsResult.value);
        } else {
          setError(settingsResult.reason?.message || 'Could not load settings.');
        }
        if (dlnaResult.status === 'fulfilled') setDlna(dlnaResult.value);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  const updateSetting = async (key: string, value: string) => {
    const previous = settings[key];
    setSettings((current) => ({ ...current, [key]: value }));
    setBusyKey(key);
    setError('');
    setSaveStatus('');
    try {
      await api.settings.update({ [key]: value });
      setSaveStatus('Saved');
    } catch (updateError: any) {
      setSettings((current) => ({ ...current, [key]: previous }));
      setError(updateError?.message || 'Could not save setting.');
    } finally {
      setBusyKey(null);
    }
  };

  const updateCustomColor = (key: ThemeColorKey, value: string) => {
    if (!appSettings) return;
    onHybridThemeModeChange?.('custom');
    onAppSettingsChange?.({ ...appSettings, [key]: value });
  };

  return (
    <MobileBottomSheet title="Settings" onClose={onClose}>
      <div style={styles.content}>
        <div style={styles.profile}>
          <div aria-hidden="true" style={styles.avatar}>
            {currentUser.username.slice(0, 2).toUpperCase()}
          </div>
          <div style={styles.profileMeta}>
            <div style={styles.profileName}>{currentUser.username}</div>
            <div style={styles.profileRole}>{currentUser.role} profile</div>
          </div>
          <span style={styles.versionBadge}>v{APP_VERSION}</span>
        </div>

        {loading ? <div role="status" style={styles.feedback}>Loading settings...</div> : null}
        {saveStatus ? <div role="status" style={styles.saved}>{saveStatus}</div> : null}
        {error ? <div role="alert" style={styles.error}>{error}</div> : null}

        <section aria-labelledby="mobile-settings-appearance" style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <div style={styles.eyebrow}>Personal</div>
              <h3 id="mobile-settings-appearance" style={styles.heading}>Appearance</h3>
            </div>
            <span style={styles.sectionBadge}>New</span>
          </div>
          <p style={styles.sectionDescription}>Choose a supported theme or use your saved custom palette.</p>
          <div role="group" aria-label="New design theme mode" style={styles.modeGrid}>
            {HYBRID_THEME_MODES.map((mode) => {
              const active = hybridThemeMode === mode;
              return (
                <button
                  key={mode}
                  type="button"
                  aria-label={`Use ${mode} theme`}
                  aria-pressed={active}
                  style={{ ...styles.modeButton, ...(active ? styles.modeButtonActive : null) }}
                  onClick={() => onHybridThemeModeChange?.(mode)}
                >
                  {mode.charAt(0).toUpperCase() + mode.slice(1)}
                </button>
              );
            })}
          </div>
          {hybridThemeMode === 'custom' && appSettings ? (
            <div style={styles.palette}>
              {THEME_COLOR_FIELDS.map(({ key, label }) => (
                <label key={key} style={styles.colorField}>
                  <span style={styles.colorLabel}>{label}</span>
                  <span style={styles.colorControl}>
                    <input
                      type="color"
                      aria-label={`${label} color`}
                      value={appSettings[key]}
                      onChange={(event) => updateCustomColor(key, event.target.value)}
                      style={styles.colorInput}
                    />
                    <span style={styles.colorValue}>{appSettings[key].toUpperCase()}</span>
                  </span>
                </label>
              ))}
            </div>
          ) : null}
          <MobileSwitch
            label="Adaptive accent"
            description={adaptiveAccentEnabled
              ? 'Following album and artist artwork.'
              : 'Using the selected theme accent.'}
            checked={adaptiveAccentEnabled}
            onChange={(enabled) => onAdaptiveAccentEnabledChange?.(enabled)}
          />
        </section>

        <section aria-labelledby="mobile-settings-playback" style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <div style={styles.eyebrow}>Audio</div>
              <h3 id="mobile-settings-playback" style={styles.heading}>Playback</h3>
            </div>
          </div>
          <MobileSwitch
            label="Stream Direct"
            description="Play supported files without transcoding."
            checked={streamDirect}
            onChange={(enabled) => {
              setStreamDirect(enabled);
              setStreamDirectState(enabled);
            }}
          />
          <MobileVolumeLevelling prefs={loudness} onChange={(change) => onLoudnessChange?.(change)} />
          <MobileSwitch
            label="Crossfade"
            description="Blend track transitions automatically."
            checked={settings.crossfadeMode === 'auto'}
            onChange={(enabled) => void updateSetting('crossfadeMode', enabled ? 'auto' : 'off')}
          />
        </section>

        <section aria-labelledby="mobile-settings-quality" style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <div style={styles.eyebrow}>Network</div>
              <h3 id="mobile-settings-quality" style={styles.heading}>Quality</h3>
            </div>
          </div>
          <label style={styles.selectRow}>
            <span style={styles.settingCopy}>
              <span style={styles.settingLabel}>Transcode quality</span>
              <span style={styles.settingDescription}>Used when direct streaming is unavailable.</span>
            </span>
            <select
              aria-label="Transcode quality"
              value={settings.transcodeQuality || 'low'}
              disabled={busyKey === 'transcodeQuality'}
              onChange={(event) => void updateSetting('transcodeQuality', event.target.value)}
              style={styles.select}
            >
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
              <option value="lossless">Lossless</option>
            </select>
          </label>
        </section>

        <section aria-labelledby="mobile-settings-server" style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <div style={styles.eyebrow}>Server</div>
              <h3 id="mobile-settings-server" style={styles.heading}>DLNA</h3>
            </div>
            <span style={{
              ...styles.statusBadge,
              ...(dlna?.running ? styles.statusBadgeActive : null),
            }}>
              {dlna?.running ? 'Running' : 'Stopped'}
            </span>
          </div>
          <div style={styles.serverRow}>
            <span>{dlna?.friendlyName || 'BoogieBox'}</span>
            <span>{dlna
              ? `${dlna.running ? 'Running' : 'Stopped'}${dlna.port ? ` on ${dlna.port}` : ''}`
              : 'Status unavailable'}</span>
          </div>
        </section>

        <MobileConnectedApps username={currentUser.username} />
        <MobileScrobbling />

        {currentUser.role === 'admin' ? <MobileAdminSettingsPanel focus={focus} /> : null}

        <section aria-labelledby="mobile-settings-account" style={styles.section}>
          <div style={styles.sectionHeader}>
            <div>
              <div style={styles.eyebrow}>Session</div>
              <h3 id="mobile-settings-account" style={styles.heading}>Account</h3>
            </div>
          </div>
          <button
            type="button"
            style={styles.logoutButton}
            onClick={() => api.auth.logout().then(() => window.location.reload())}
          >
            Log out of {currentUser.username}
          </button>
        </section>
      </div>
    </MobileBottomSheet>
  );
}

const styles: Record<string, React.CSSProperties> = {
  content: { display: 'grid', gap: 12 },
  profile: {
    minHeight: 66,
    display: 'grid',
    gridTemplateColumns: '48px minmax(0, 1fr) auto',
    alignItems: 'center',
    gap: 10,
    padding: '8px 10px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  avatar: {
    width: 48,
    height: 48,
    display: 'grid',
    placeItems: 'center',
    borderRadius: 12,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 16,
    fontWeight: 800,
  },
  profileMeta: { minWidth: 0 },
  profileName: {
    overflow: 'hidden',
    color: 'var(--text)',
    fontSize: 16,
    fontWeight: 750,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  profileRole: {
    marginTop: 3,
    color: 'var(--text-muted)',
    fontSize: 12,
    fontWeight: 600,
    textTransform: 'capitalize',
  },
  versionBadge: {
    padding: '4px 7px',
    borderRadius: 999,
    background: 'var(--surface)',
    color: 'var(--text-faint)',
    fontSize: 11,
    fontWeight: 700,
  },
  feedback: hybridMobileContentStyles.feedback,
  saved: {
    ...hybridMobileContentStyles.feedback,
    borderColor: 'color-mix(in srgb, var(--success) 32%, var(--divider-subtle))',
    background: 'color-mix(in srgb, var(--success) 9%, var(--surface))',
    color: 'var(--success)',
  },
  error: {
    ...hybridMobileContentStyles.feedback,
    ...hybridMobileContentStyles.feedbackError,
  },
  section: {
    display: 'grid',
    gap: 10,
    padding: 12,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'var(--surface)',
  },
  sectionHeader: {
    minHeight: 38,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
  },
  eyebrow: hybridMobileContentStyles.eyebrow,
  heading: {
    margin: '3px 0 0',
    color: 'var(--text)',
    fontSize: 19,
    fontWeight: 800,
    letterSpacing: -0.25,
  },
  sectionDescription: {
    margin: '-2px 0 0',
    color: 'var(--text-muted)',
    fontSize: 12,
    lineHeight: 1.5,
  },
  sectionBadge: {
    padding: '4px 7px',
    borderRadius: 999,
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontSize: 11,
    fontWeight: 750,
  },
  modeGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
    gap: 6,
    padding: 4,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 13,
    background: 'var(--surface-subtle)',
  },
  modeButton: {
    minWidth: 0,
    minHeight: 44,
    padding: '0 8px',
    border: 'none',
    borderRadius: 10,
    background: 'transparent',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 700,
  },
  modeButtonActive: {
    background: 'var(--surface-raised)',
    color: 'var(--accent)',
    boxShadow: 'var(--shadow-subtle)',
  },
  palette: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 8,
  },
  colorField: {
    minWidth: 0,
    display: 'grid',
    gap: 5,
    color: 'var(--text-muted)',
    fontSize: 12,
    fontWeight: 650,
  },
  colorLabel: { paddingLeft: 2 },
  colorControl: {
    minHeight: 44,
    display: 'grid',
    gridTemplateColumns: '44px minmax(0, 1fr)',
    alignItems: 'center',
    overflow: 'hidden',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
  },
  colorInput: {
    width: 44,
    height: 44,
    padding: 4,
    border: 'none',
    background: 'transparent',
  },
  colorValue: {
    overflow: 'hidden',
    color: 'var(--text-muted)',
    fontSize: 11,
    fontWeight: 700,
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  settingRow: {
    minHeight: 58,
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) 48px',
    alignItems: 'center',
    gap: 10,
    paddingTop: 8,
    borderTop: '1px solid var(--divider-subtle)',
  },
  settingCopy: { minWidth: 0, display: 'grid', gap: 3 },
  settingLabel: { color: 'var(--text)', fontSize: 14, fontWeight: 750 },
  settingDescription: { color: 'var(--text-muted)', fontSize: 12, lineHeight: 1.4 },
  switchButton: {
    width: 48,
    minWidth: 48,
    minHeight: 44,
    display: 'grid',
    placeItems: 'center',
    padding: 0,
    border: 'none',
    background: 'transparent',
  },
  switchTrack: {
    width: 44,
    height: 24,
    position: 'relative',
    display: 'block',
    borderRadius: 999,
    background: 'var(--border-strong)',
  },
  switchTrackActive: { background: 'var(--accent)' },
  switchThumb: {
    width: 18,
    height: 18,
    position: 'absolute',
    top: 3,
    left: 3,
    borderRadius: '50%',
    background: 'var(--surface-raised)',
    boxShadow: 'var(--shadow-subtle)',
    transition: 'transform 160ms ease',
  },
  switchThumbActive: { transform: 'translateX(20px)' },
  selectRow: {
    minHeight: 58,
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) 120px',
    alignItems: 'center',
    gap: 10,
    paddingTop: 8,
    borderTop: '1px solid var(--divider-subtle)',
  },
  select: {
    width: '100%',
    minHeight: 44,
    padding: '0 10px',
    border: '1px solid var(--divider-subtle)',
    borderRadius: 11,
    background: 'var(--surface-subtle)',
    color: 'var(--text)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 700,
  },
  statusBadge: {
    padding: '4px 7px',
    borderRadius: 999,
    background: 'var(--surface-subtle)',
    color: 'var(--text-faint)',
    fontSize: 11,
    fontWeight: 750,
  },
  statusBadgeActive: {
    background: 'color-mix(in srgb, var(--success) 12%, var(--surface))',
    color: 'var(--success)',
  },
  serverRow: {
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    paddingTop: 8,
    borderTop: '1px solid var(--divider-subtle)',
    color: 'var(--text-muted)',
    fontSize: 12,
  },
  levelling: {
    display: 'grid',
    gap: 10,
    paddingTop: 8,
    borderTop: '1px solid var(--divider-subtle)',
  },
  levellingTitleRow: { display: 'flex', alignItems: 'center', gap: 8 },
  levellingGrid: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: 8,
  },
  levellingChip: {
    minWidth: 0,
    minHeight: 44,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 10,
    background: 'transparent',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 650,
  },
  levellingChipActive: {
    borderColor: 'var(--accent)',
    background: 'var(--accent-soft)',
    color: 'var(--accent)',
    fontWeight: 750,
  },
  preampRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    paddingTop: 10,
    borderTop: '1px solid var(--divider-subtle)',
  },
  stepper: { display: 'flex', alignItems: 'center', gap: 6 },
  stepperButton: {
    width: 44,
    height: 44,
    display: 'grid',
    placeItems: 'center',
    padding: 0,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 22,
    background: 'transparent',
    color: 'var(--text)',
  },
  stepperValue: {
    minWidth: 64,
    textAlign: 'center',
    color: 'var(--text)',
    fontSize: 14,
    fontVariantNumeric: 'tabular-nums',
  },
  logoutButton: {
    minHeight: 44,
    border: '1px solid color-mix(in srgb, var(--danger) 28%, var(--divider-subtle))',
    borderRadius: 11,
    background: 'color-mix(in srgb, var(--danger) 8%, var(--surface))',
    color: 'var(--danger)',
    fontFamily: 'inherit',
    fontSize: 13,
    fontWeight: 750,
  },
};
