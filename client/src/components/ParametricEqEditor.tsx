/**
 * Defines the Parametric Eq Editor React component and related UI helpers.
 */

import React, { useState } from 'react';
import {
  BUILTIN_PARAMETRIC_PRESETS, BUILTIN_PARAMETRIC_PRESET_NAMES,
  isBuiltinParametricPresetName,
  PARAMETRIC_DB_MIN, PARAMETRIC_DB_MAX, PARAMETRIC_FREQ_MIN, PARAMETRIC_FREQ_MAX,
  PARAMETRIC_Q_MIN, PARAMETRIC_Q_MAX,
  clampDb, clampFreq, clampQ,
  type ParametricEqBand, type ParametricEqProfile, type ParametricEqBandType,
} from '../audio/eq';
import { hybridControlStyles } from '../hybridPreview';
import EqCurveCanvas from './EqCurveCanvas';

const BAND_TYPES: { value: ParametricEqBandType; label: string }[] = [
  { value: 'peaking',    label: 'Peaking'   },
  { value: 'low_shelf',  label: 'Low Shelf' },
  { value: 'high_shelf', label: 'High Shelf'},
  { value: 'highpass',   label: 'High Pass' },
  { value: 'lowpass',    label: 'Low Pass'  },
];

const STYLE = {
  root: { display: 'flex', flexDirection: 'column' as const, gap: 10 },
  profileRow: { display: 'flex', alignItems: 'center', gap: 6 },
  select: {
    ...hybridControlStyles.select,
    flex: 1,
    minHeight: 34,
    padding: '6px 30px 6px 9px',
    fontSize: 11,
  },
  input: {
    ...hybridControlStyles.field,
    flex: 1,
    minWidth: 120,
    minHeight: 34,
    padding: '6px 9px',
    fontSize: 11,
  },
  btn: {
    ...hybridControlStyles.secondaryButton,
    minHeight: 34,
    padding: '6px 10px',
    fontSize: 11,
  },
  status: { fontSize: 10, color: 'var(--text-muted)' },
  bandStrip: { display: 'flex', gap: 4, flexWrap: 'nowrap' as const },
  bandChip: (active: boolean, enabled: boolean): React.CSSProperties => ({
    flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 1,
    padding: '6px 2px', borderRadius: 9, cursor: 'pointer', fontSize: 9,
    border: '1px solid transparent',
    backgroundColor: active ? 'var(--accent-soft)' : 'var(--surface-subtle)',
    opacity: enabled ? 1 : 0.45,
    color: active ? 'var(--accent)' : 'var(--text-muted)',
    fontFamily: 'inherit',
    lineHeight: 1.2,
  }),
  controlRow: { display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' as const },
  controlLabel: { fontSize: 10, color: 'var(--text-muted)', whiteSpace: 'nowrap' as const, minWidth: 28 },
  numberInput: {
    ...hybridControlStyles.field,
    width: 72,
    minHeight: 32,
    padding: '5px 7px',
    fontSize: 11,
  },
  smallSelect: {
    ...hybridControlStyles.select,
    minHeight: 32,
    padding: '5px 28px 5px 8px',
    fontSize: 11,
  },
} as const;

/** Parametric Eq Editor Props is part of this module's public API. */
export interface ParametricEqEditorProps {
  bands: ParametricEqBand[];
  profile: string;
  customProfiles: ParametricEqProfile[];
  autoEqEnabled: boolean;
  newProfileName: string;
  accentColor: string;
  onBandsChange: (bands: ParametricEqBand[]) => void;
  onProfileChange: (name: string, bands: ParametricEqBand[]) => void;
  onNewProfileNameChange: (name: string) => void;
  onSaveProfile: (name: string, bands: ParametricEqBand[]) => Promise<string | null>;
  onDeleteProfile: (name: string) => Promise<void>;
}

/** Parametric Eq Editor is part of this module's public API. */
export default function ParametricEqEditor({
  bands,
  profile,
  customProfiles,
  autoEqEnabled,
  newProfileName,
  accentColor,
  onBandsChange,
  onProfileChange,
  onNewProfileNameChange,
  onSaveProfile,
  onDeleteProfile,
}: ParametricEqEditorProps) {
  const [selectedBandIndex, setSelectedBandIndex] = useState(0);
  const [status, setStatus] = useState<string | null>(null);

  const selectedBand = bands[selectedBandIndex];
  const isCustomProfile = !isBuiltinParametricPresetName(profile);

  const updateBand = (index: number, updates: Partial<ParametricEqBand>) => {
    const next = bands.map((b, i) => (i === index ? { ...b, ...updates } : b));
    onBandsChange(next);
  };

  const handleProfileSelect = (name: string) => {
    const builtinBands = BUILTIN_PARAMETRIC_PRESETS[name];
    if (builtinBands) { onProfileChange(name, builtinBands); return; }
    const custom = customProfiles.find((p) => p.name === name);
    if (custom) onProfileChange(name, custom.bands);
  };

  const handleSave = async () => {
    const name = newProfileName.trim();
    const err = await onSaveProfile(name, bands);
    if (err) { setStatus(err); } else { setStatus('Profile saved.'); }
  };

  const handleDelete = async () => {
    if (!isCustomProfile) return;
    await onDeleteProfile(profile);
    setStatus('Profile deleted.');
  };

  const hasGain = selectedBand?.type !== 'highpass' && selectedBand?.type !== 'lowpass';

  return (
    <div data-ui-region="parametric-equalizer" style={STYLE.root}>
      {/* Profile selector */}
      <div style={STYLE.profileRow}>
        <select
          aria-label="EQ profile"
          style={STYLE.select}
          value={profile}
          disabled={autoEqEnabled}
          onChange={(e) => handleProfileSelect(e.currentTarget.value)}
        >
          {BUILTIN_PARAMETRIC_PRESET_NAMES.map((n) => (
            <option key={n} value={n}>{n}</option>
          ))}
          {customProfiles.map((p) => (
            <option key={p.name} value={p.name}>{`Custom: ${p.name}`}</option>
          ))}
        </select>
        <button
          style={STYLE.btn}
          disabled={autoEqEnabled || !isCustomProfile}
          onClick={handleDelete}
          title="Delete selected custom profile"
        >Delete</button>
      </div>

      {/* Save row */}
      <div style={STYLE.profileRow}>
        <input
          aria-label="New EQ profile name"
          style={STYLE.input}
          placeholder="Save as..."
          value={newProfileName}
          disabled={autoEqEnabled}
          onChange={(e) => { onNewProfileNameChange(e.currentTarget.value); setStatus(null); }}
        />
        <button style={STYLE.btn} disabled={autoEqEnabled} onClick={handleSave}>Save</button>
      </div>
      {status && <div style={STYLE.status}>{status}</div>}

      {/* Curve canvas */}
      <EqCurveCanvas
        bands={bands}
        selectedBandIndex={selectedBandIndex}
        accentColor={accentColor}
        onChange={(index, updates) => updateBand(index, updates)}
        onSelectBand={setSelectedBandIndex}
        width={536}
        height={140}
      />

      {/* Band chips */}
      <div style={STYLE.bandStrip} role="group" aria-label="EQ bands">
        {bands.map((band, i) => {
          const freqLabel = band.frequencyHz >= 1000 ? `${(band.frequencyHz / 1000).toFixed(band.frequencyHz % 1000 === 0 ? 0 : 1)}k` : `${band.frequencyHz}`;
          const gainLabel = (band.type !== 'highpass' && band.type !== 'lowpass') ? `${band.gainDb >= 0 ? '+' : ''}${band.gainDb.toFixed(1)}` : '--';
          return (
            <button
              key={band.id}
              style={STYLE.bandChip(i === selectedBandIndex, band.enabled)}
              onClick={() => setSelectedBandIndex(i)}
              aria-pressed={i === selectedBandIndex}
              aria-label={`Band ${i + 1} ${band.label}, ${freqLabel} Hz, ${gainLabel} dB`}
            >
              <span style={{ fontWeight: 700, fontSize: 10 }}>{i + 1}</span>
              <span>{band.label}</span>
              <span>{freqLabel}</span>
              <span>{gainLabel}</span>
            </button>
          );
        })}
      </div>

      {/* Selected band controls */}
      {selectedBand && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, padding: '10px 0 2px', borderTop: '1px solid var(--divider-subtle)' }}>
          <div style={STYLE.controlRow}>
            <span style={STYLE.controlLabel}>Type</span>
            <select
              aria-label={`Band ${selectedBandIndex + 1} type`}
              style={STYLE.smallSelect}
              value={selectedBand.type}
              disabled={autoEqEnabled}
              onChange={(e) => updateBand(selectedBandIndex, { type: e.currentTarget.value as ParametricEqBandType })}
            >
              {BAND_TYPES.map(({ value, label }) => (
                <option key={value} value={value}>{label}</option>
              ))}
            </select>
            <label style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 10, color: 'var(--text-muted)', marginLeft: 'auto', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={selectedBand.enabled}
                disabled={autoEqEnabled}
                onChange={(e) => updateBand(selectedBandIndex, { enabled: e.currentTarget.checked })}
              />
              Enabled
            </label>
          </div>

          <div style={STYLE.controlRow}>
            <span style={STYLE.controlLabel}>Freq</span>
            <input
              aria-label={`Band ${selectedBandIndex + 1} frequency`}
              type="number"
              style={STYLE.numberInput}
              min={PARAMETRIC_FREQ_MIN}
              max={PARAMETRIC_FREQ_MAX}
              step={1}
              value={Math.round(selectedBand.frequencyHz)}
              disabled={autoEqEnabled}
              onChange={(e) => {
                const v = Number(e.currentTarget.value);
                if (Number.isFinite(v)) updateBand(selectedBandIndex, { frequencyHz: clampFreq(v) });
              }}
            />
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>Hz</span>

            <span style={{ ...STYLE.controlLabel, marginLeft: 8 }}>Q</span>
            <input
              aria-label={`Band ${selectedBandIndex + 1} Q`}
              type="number"
              style={STYLE.numberInput}
              min={PARAMETRIC_Q_MIN}
              max={PARAMETRIC_Q_MAX}
              step={0.1}
              value={selectedBand.q.toFixed(1)}
              disabled={autoEqEnabled}
              onChange={(e) => {
                const v = Number(e.currentTarget.value);
                if (Number.isFinite(v)) updateBand(selectedBandIndex, { q: clampQ(v) });
              }}
            />
          </div>

          <div style={STYLE.controlRow}>
            <span style={STYLE.controlLabel}>Gain</span>
            <input
              aria-label={`Band ${selectedBandIndex + 1} gain value`}
              type="number"
              style={{ ...STYLE.numberInput, opacity: hasGain ? 1 : 0.4 }}
              min={PARAMETRIC_DB_MIN}
              max={PARAMETRIC_DB_MAX}
              step={0.5}
              value={selectedBand.gainDb.toFixed(1)}
              disabled={autoEqEnabled || !hasGain}
              onChange={(e) => {
                const v = Number(e.currentTarget.value);
                if (Number.isFinite(v)) updateBand(selectedBandIndex, { gainDb: clampDb(v) });
              }}
            />
            <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>dB</span>
            <input
              type="range"
              min={PARAMETRIC_DB_MIN}
              max={PARAMETRIC_DB_MAX}
              step={0.5}
              value={selectedBand.gainDb}
              disabled={autoEqEnabled || !hasGain}
              style={{ flex: 1, opacity: hasGain ? 1 : 0.4, accentColor }}
              onChange={(e) => updateBand(selectedBandIndex, { gainDb: clampDb(Number(e.currentTarget.value)) })}
              aria-label={`Band ${selectedBandIndex + 1} gain`}
            />
            <span style={{ fontSize: 10, color: 'var(--text-muted)', minWidth: 32, textAlign: 'right' }}>
              {selectedBand.gainDb >= 0 ? '+' : ''}{selectedBand.gainDb.toFixed(1)} dB
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
