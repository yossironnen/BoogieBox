/**
 * Volume levelling settings (wip/loudness-levelling-plan.md §8): the per-user card (mode chips, pre-amp, where
 * levelling applies) and the admin "Server levelling" card (Play On and Subsonic modes, the measuring lane).
 * The mode icons are shared with the mobile settings sheet.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import { LOUDNESS_MODES, type LoudnessMode, type LoudnessPrefs } from '../audio/loudness';
import { hybridControlStyles, hybridSettingsStyles } from '../hybridPreview';
import type { LoudnessStatus } from '../types';
import { InlineStatus, ToggleSwitch } from './settingsControls';

const ICON_PROPS = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
};

export const LOUDNESS_MODE_LABELS: Record<LoudnessMode, string> = {
  off: 'Off',
  track: 'Track',
  album: 'Album',
  auto: 'Auto',
};

export const LOUDNESS_MODE_HINTS: Record<LoudnessMode, string> = {
  off: 'Tracks play at their own volume.',
  track: 'Every track plays at the same loudness.',
  album: 'Albums keep their own quiet and loud tracks.',
  auto: 'Album levels when you play an album in order, track levels when you shuffle.',
};

export const PREAMP_NOTE =
  'Levelled tracks play quieter than most modern recordings. Raise the pre-amp to bring them up; peaks are never pushed into clipping.';

export const PREAMP_MIN_DB = -6;
export const PREAMP_MAX_DB = 6;
export const PREAMP_STEP_DB = 0.5;

/** "+1.5 dB", "0 dB", "−2 dB". */
export function formatPreamp(db: number): string {
  if (db === 0) return '0 dB';
  return `${db > 0 ? '+' : '−'}${Math.abs(db)} dB`;
}

export function LoudnessModeIcon({ mode, size = 16 }: { mode: LoudnessMode; size?: number }) {
  const paths: Record<LoudnessMode, React.ReactElement> = {
    off: <><circle cx="12" cy="12" r="9" /><path d="M5.6 5.6l12.8 12.8" /></>,
    track: <><path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" /></>,
    album: <><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="2.5" /></>,
    auto: <><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1" /></>,
  };
  return <svg width={size} height={size} {...ICON_PROPS} style={{ flexShrink: 0 }}>{paths[mode]}</svg>;
}

/** The sliders glyph that heads both levelling cards. */
export function LevellingIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} {...ICON_PROPS} stroke="var(--accent)" style={{ opacity: 0.85, flexShrink: 0 }}>
      <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
    </svg>
  );
}

function StatIcon({ children }: { children: React.ReactNode }) {
  return (
    <svg width={18} height={18} {...ICON_PROPS} stroke="var(--accent)" style={{ opacity: 0.85, flexShrink: 0 }}>
      {children}
    </svg>
  );
}

function ModeChips({ value, modes, onChange }: {
  value: LoudnessMode;
  modes: readonly LoudnessMode[];
  onChange: (mode: LoudnessMode) => void;
}) {
  return (
    <div role="group" aria-label="Volume levelling mode" style={hybridControlStyles.segmentedGroup}>
      {modes.map(mode => {
        const active = value === mode;
        return (
          <button
            key={mode}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(mode)}
            style={{
              ...hybridControlStyles.segment,
              ...(active ? hybridControlStyles.segmentActive : {}),
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
            }}
          >
            <LoudnessModeIcon mode={mode} />
            {LOUDNESS_MODE_LABELS[mode]}
          </button>
        );
      })}
    </div>
  );
}

const PLACES: ReadonlyArray<{ key: 'player' | 'playOn' | 'subsonic' | 'dlna'; title: string; icon: React.ReactElement }> = [
  { key: 'player', title: 'This player', icon: <><rect x="2" y="4" width="20" height="14" rx="2" /><path d="M8 21h8" /></> },
  { key: 'playOn', title: 'Play On speakers', icon: <><rect x="5" y="2" width="14" height="20" rx="2" /><circle cx="12" cy="14" r="4" /><circle cx="12" cy="6" r="1" /></> },
  { key: 'subsonic', title: 'Subsonic apps', icon: <><rect x="6" y="2" width="12" height="20" rx="2" /><path d="M11 18h2" /></> },
  { key: 'dlna', title: 'DLNA and radio', icon: <><path d="M4.9 19.1a10 10 0 0 1 0-14.2M19.1 4.9a10 10 0 0 1 0 14.2M7.8 16.2a6 6 0 0 1 0-8.4M16.2 7.8a6 6 0 0 1 0 8.4" /><circle cx="12" cy="12" r="2" /></> },
];

/** Settings → User Settings: this user's levelling mode and pre-amp. */
export function VolumeLevellingCard({ prefs, onChange }: {
  prefs: LoudnessPrefs;
  onChange: (change: Partial<LoudnessPrefs>) => void;
}) {
  const [playOnMode, setPlayOnMode] = useState<string>('off');
  useEffect(() => {
    let active = true;
    api.playbackSettings()
      .then(shared => { if (active) setPlayOnMode(shared.loudnessPlayOnMode ?? 'off'); })
      .catch(() => {});
    return () => { active = false; };
  }, []);

  const status: Record<(typeof PLACES)[number]['key'], string> = {
    player: prefs.mode === 'off' ? 'Not levelled' : 'Levelled',
    playOn: playOnMode === 'off' ? 'Not levelled (set by an admin)' : 'Levelled (set by an admin)',
    subsonic: 'The app levels with the values it gets',
    dlna: 'Not levelled',
  };

  return (
    <section aria-labelledby="volume-levelling-title" style={hybridSettingsStyles.panel}>
      <div style={styles.titleRow}>
        <LevellingIcon />
        <div id="volume-levelling-title" style={hybridSettingsStyles.sectionTitle}>Volume levelling</div>
      </div>
      <div style={styles.description}>
        Plays tracks at a similar volume without changing how they sound. Uses ReplayGain tags, or values the server
        measures for tracks without them.
      </div>

      <ModeChips value={prefs.mode} modes={LOUDNESS_MODES} onChange={mode => onChange({ mode })} />
      <div style={styles.hint}>{LOUDNESS_MODE_HINTS[prefs.mode]}</div>

      <div style={styles.preampRow}>
        <label htmlFor="loudness-preamp" style={styles.preampLabel}>Pre-amp</label>
        <input
          id="loudness-preamp"
          type="range"
          min={PREAMP_MIN_DB}
          max={PREAMP_MAX_DB}
          step={PREAMP_STEP_DB}
          value={prefs.preampDb}
          aria-valuetext={formatPreamp(prefs.preampDb)}
          onChange={e => onChange({ preampDb: Number(e.target.value) })}
          style={styles.range}
        />
        <span style={styles.preampValue}>{formatPreamp(prefs.preampDb)}</span>
      </div>
      <div style={styles.note}>{PREAMP_NOTE}</div>

      <div style={styles.placesBlock}>
        <div style={styles.placesTitle}>Where levelling applies</div>
        <div style={styles.placesGrid}>
          {PLACES.map(place => (
            <div key={place.key} style={styles.tile}>
              <StatIcon>{place.icon}</StatIcon>
              <div>
                <div style={styles.tileTitle}>{place.title}</div>
                <div style={styles.tileText}>{status[place.key]}</div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

const PLAY_ON_MODES: readonly LoudnessMode[] = LOUDNESS_MODES;
const SUBSONIC_MODES: readonly LoudnessMode[] = ['off', 'track', 'album'];

const STAT_TILES: ReadonlyArray<{ key: 'tagged' | 'measured' | 'remaining' | 'failed'; label: string; icon: React.ReactElement }> = [
  { key: 'tagged', label: 'From tags', icon: <><path d="M20.6 13.4l-7.2 7.2a2 2 0 0 1-2.8 0L2 12V2h10l8.6 8.6a2 2 0 0 1 0 2.8z" /><circle cx="7" cy="7" r="1.5" /></> },
  { key: 'measured', label: 'Measured', icon: <path d="M2 12h3l3-8 4 16 3-8h7" /> },
  { key: 'remaining', label: 'Waiting', icon: <><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></> },
  { key: 'failed', label: 'Couldn’t measure', icon: <><path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /><path d="M12 9v4M12 17h.01" /></> },
];

/** Settings → Advanced → Playback (admin): server-side levelling and the measuring lane. */
export function ServerLevellingCard() {
  const [playOnMode, setPlayOnMode] = useState<LoudnessMode>('off');
  const [subsonicMode, setSubsonicMode] = useState<LoudnessMode>('off');
  const [measuring, setMeasuring] = useState(true);
  const [status, setStatus] = useState<LoudnessStatus | null>(null);
  const [result, setResult] = useState<string | null>(null);

  const loadStatus = useCallback(() => {
    api.admin.loudnessStatus().then(setStatus).catch(() => {});
  }, []);

  useEffect(() => {
    api.settings.get().then(s => {
      const pick = (value: string | undefined, allowed: readonly LoudnessMode[]) =>
        allowed.includes(value as LoudnessMode) ? (value as LoudnessMode) : 'off';
      setPlayOnMode(pick(s.loudnessPlayOnMode, PLAY_ON_MODES));
      setSubsonicMode(pick(s.loudnessSubsonicTranscodeMode, SUBSONIC_MODES));
      setMeasuring((s.loudnessAnalysisEnabled ?? 'true') !== 'false');
    }).catch(() => {});
    loadStatus();
    const timer = window.setInterval(loadStatus, 15000);
    return () => window.clearInterval(timer);
  }, [loadStatus]);

  const save = async (key: string, value: string) => {
    setResult(null);
    try {
      await api.settings.update({ [key]: value });
      setResult('Saved');
      if (key === 'loudnessAnalysisEnabled') loadStatus();
    } catch (e: any) {
      setResult(`Error: ${e?.message || 'Could not save.'}`);
    }
  };

  const modeSelect = (id: string, value: LoudnessMode, modes: readonly LoudnessMode[], onPick: (mode: LoudnessMode) => void) => (
    <select id={id} value={value} onChange={e => onPick(e.target.value as LoudnessMode)} style={hybridControlStyles.select}>
      {modes.map(mode => <option key={mode} value={mode}>{LOUDNESS_MODE_LABELS[mode]}</option>)}
    </select>
  );

  return (
    <section aria-labelledby="server-levelling-title" style={hybridSettingsStyles.panel}>
      <div style={styles.titleRow}>
        <StatIcon><rect x="2" y="3" width="20" height="8" rx="2" /><rect x="2" y="13" width="20" height="8" rx="2" /><path d="M6 7h.01M6 17h.01" /></StatIcon>
        <div id="server-levelling-title" style={hybridSettingsStyles.sectionTitle}>Server levelling</div>
        <span style={styles.badge}>Admin</span>
      </div>

      <div style={styles.twoColumns}>
        <div style={styles.field}>
          <label htmlFor="loudness-play-on-mode" style={styles.fieldLabel}>Play On speakers</label>
          {modeSelect('loudness-play-on-mode', playOnMode, PLAY_ON_MODES, mode => {
            setPlayOnMode(mode);
            void save('loudnessPlayOnMode', mode);
          })}
          <div style={styles.note}>
            Levelled tracks are converted on the fly (lossless stays lossless), so they use more network bandwidth.
          </div>
        </div>
        <div style={styles.field}>
          <label htmlFor="loudness-subsonic-mode" style={styles.fieldLabel}>Subsonic transcodes</label>
          {modeSelect('loudness-subsonic-mode', subsonicMode, SUBSONIC_MODES, mode => {
            setSubsonicMode(mode);
            void save('loudnessSubsonicTranscodeMode', mode);
          })}
          <div style={styles.note}>
            Most apps level with the values they get. Turn this on only for apps that don't, or tracks are levelled twice.
          </div>
        </div>
      </div>

      <div style={styles.switchRow}>
        <div>
          <div style={styles.fieldLabel}>Measure tracks without tags</div>
          <div style={{ ...styles.note, marginTop: 4 }}>
            Measures loudness in the background, one track at a time. Pauses while a BoogieMix renders.
          </div>
        </div>
        <ToggleSwitch
          checked={measuring}
          ariaLabel="Measure tracks without tags"
          onChange={() => {
            const next = !measuring;
            setMeasuring(next);
            void save('loudnessAnalysisEnabled', String(next));
          }}
        />
      </div>

      <div style={styles.statGrid}>
        {STAT_TILES.map(tile => (
          <div key={tile.key} style={styles.tile} aria-label={`${tile.label}: ${status ? status[tile.key] : 'unknown'}`}>
            <StatIcon>{tile.icon}</StatIcon>
            <div>
              <div style={styles.statValue}>{status ? status[tile.key].toLocaleString() : '–'}</div>
              <div style={styles.tileText}>{tile.label}</div>
            </div>
          </div>
        ))}
      </div>
      <InlineStatus busy={false} busyText="" result={result} />
    </section>
  );
}

const styles: Record<string, React.CSSProperties> = {
  titleRow: { display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8 },
  description: { fontSize: 14, color: 'var(--text-muted)', lineHeight: 1.6, marginBottom: 14 },
  hint: { fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.5, marginTop: 8 },
  preampRow: { display: 'flex', alignItems: 'center', gap: 14, marginTop: 16, flexWrap: 'wrap' },
  preampLabel: { fontSize: 14, fontWeight: 600, color: 'var(--text)', minWidth: 64 },
  range: { flex: '1 1 180px', maxWidth: 360, accentColor: 'var(--accent)' },
  preampValue: { fontSize: 14, color: 'var(--text)', minWidth: 56, fontVariantNumeric: 'tabular-nums' },
  note: { fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.5, marginTop: 6 },
  placesBlock: { borderTop: '1px solid var(--divider-subtle)', marginTop: 16, paddingTop: 12 },
  placesTitle: { fontSize: 12, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 8 },
  placesGrid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 8 },
  tile: {
    display: 'flex',
    alignItems: 'flex-start',
    gap: 10,
    padding: '10px 12px',
    borderRadius: 8,
    background: 'var(--surface-subtle)',
  },
  tileTitle: { fontSize: 13, fontWeight: 600, color: 'var(--text)' },
  tileText: { fontSize: 12, color: 'var(--text-muted)' },
  badge: {
    fontSize: 11,
    padding: '2px 8px',
    borderRadius: 10,
    border: '1px solid var(--border)',
    color: 'var(--text-muted)',
  },
  twoColumns: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 16, marginTop: 8 },
  field: { display: 'flex', flexDirection: 'column', gap: 6 },
  fieldLabel: { fontSize: 14, fontWeight: 600, color: 'var(--text)' },
  switchRow: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 16,
    borderTop: '1px solid var(--divider-subtle)',
    marginTop: 16,
    paddingTop: 14,
  },
  statGrid: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 10, marginTop: 14 },
  statValue: { fontSize: 16, fontWeight: 600, color: 'var(--text)', fontVariantNumeric: 'tabular-nums' },
};
