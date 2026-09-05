/**
 * Defines touch-safe Hybrid Equalizer and Vinyl control sheets for mobile playback.
 */

import { useState } from 'react';
import { api } from '../../api';
import ParametricEqEditor from '../../components/ParametricEqEditor';
import type { PlaybackSnapshot, PlayerEqControls } from '../../components/Player';
import VinylTurntable from '../../components/VinylTurntable';
import { hybridMobileContentStyles } from '../../hybridPreview';
import MobileBottomSheet from './MobileBottomSheet';

export function MobileEqualizerSheet({
  controls,
  onClose,
}: {
  controls: PlayerEqControls | null;
  onClose: () => void;
}) {
  const [newProfileName, setNewProfileName] = useState('');

  return (
    <MobileBottomSheet title="Equalizer" onClose={onClose} closeLabel="Close equalizer">
      <div style={S.intro}>
        <div>
          <div style={S.sectionTitle}>Parametric EQ</div>
          <div style={S.description}>Shape playback with the same profiles and seven-band audio graph used on desktop.</div>
        </div>
        <label style={S.toggleRow}>
          <span>
            <span style={S.controlTitle}>Auto EQ</span>
            <span style={S.controlMeta}>
              {controls?.autoEqEnabled
                ? `Active · ${controls.autoEqCurrentPreset}`
                : 'Choose a profile manually'}
            </span>
          </span>
          <input
            type="checkbox"
            aria-label="Auto EQ"
            checked={controls?.autoEqEnabled ?? false}
            disabled={!controls}
            onChange={(event) => controls?.onAutoEqEnabledChange(event.currentTarget.checked)}
          />
        </label>
      </div>

      {controls ? (
        <ParametricEqEditor
          bands={controls.bands}
          profile={controls.profile}
          customProfiles={controls.customProfiles}
          autoEqEnabled={controls.autoEqEnabled}
          newProfileName={newProfileName}
          accentColor="var(--accent)"
          onBandsChange={controls.onBandsChange}
          onProfileChange={controls.onProfileChange}
          onNewProfileNameChange={setNewProfileName}
          onSaveProfile={controls.onSaveProfile}
          onDeleteProfile={controls.onDeleteProfile}
          mobile
        />
      ) : (
        <div role="status" style={hybridMobileContentStyles.feedback}>
          Connecting to the playback equalizer…
        </div>
      )}
    </MobileBottomSheet>
  );
}

export function MobileVinylSheet({
  snapshot,
  playbackMode,
  vinylHardcore,
  vinylNeedleDrop,
  vinylAnalogFxDisabled,
  vinylNeedleDropIntensity,
  onPlaybackModeChange,
  onVinylHardcoreChange,
  onVinylNeedleDropChange,
  onVinylAnalogFxDisabledChange,
  onVinylNeedleDropIntensityChange,
  onClose,
}: {
  snapshot: PlaybackSnapshot | null;
  playbackMode: 'standard' | 'vinyl';
  vinylHardcore: boolean;
  vinylNeedleDrop: boolean;
  vinylAnalogFxDisabled: boolean;
  vinylNeedleDropIntensity: number;
  onPlaybackModeChange: (mode: 'standard' | 'vinyl') => void;
  onVinylHardcoreChange: (enabled: boolean) => void;
  onVinylNeedleDropChange: (enabled: boolean) => void;
  onVinylAnalogFxDisabledChange: (enabled: boolean) => void;
  onVinylNeedleDropIntensityChange: (intensity: number) => void;
  onClose: () => void;
}) {
  const track = snapshot?.currentTrack ?? null;
  const enabled = playbackMode === 'vinyl';
  const artworkUrl = track?.album_id ? api.albumArtUrl(track.album_id, 300) : null;

  return (
    <MobileBottomSheet title="Vinyl" onClose={onClose} closeLabel="Close Vinyl">
      <div style={S.vinylHero}>
        <VinylTurntable
          albumArtUrl={artworkUrl}
          title={track?.title || track?.file_name || 'Current track'}
          isPlaying={enabled && !!snapshot?.isPlaying}
          currentTime={snapshot?.currentTime ?? 0}
          duration={snapshot?.duration ?? track?.duration ?? 0}
          seekDisabled
        />
        <div style={S.vinylHeroCopy}>
          <div style={S.sectionTitle}>{enabled ? 'Vinyl mode active' : 'Standard playback'}</div>
          <div style={S.description}>
            {enabled
              ? 'Album-order playback with optional needle-drop and analog character.'
              : 'Switch modes without leaving Now Playing.'}
          </div>
        </div>
      </div>

      <div role="group" aria-label="Playback mode" style={S.modePicker}>
        {(['standard', 'vinyl'] as const).map((mode) => (
          <button
            key={mode}
            type="button"
            aria-pressed={playbackMode === mode}
            style={{
              ...S.modeButton,
              ...(playbackMode === mode ? S.modeButtonActive : null),
            }}
            onClick={() => onPlaybackModeChange(mode)}
          >
            {mode === 'standard' ? 'Standard' : 'Vinyl'}
          </button>
        ))}
      </div>

      <div style={S.controlList}>
        <MobileCheckRow
          title="Hardcore Vinyl"
          description="Disable needle seeking for a turntable-style session."
          checked={vinylHardcore}
          disabled={!enabled}
          onChange={onVinylHardcoreChange}
        />
        <MobileCheckRow
          title="Needle-drop sound"
          description="Play a brief drop effect when playback starts."
          checked={vinylNeedleDrop}
          disabled={!enabled}
          onChange={onVinylNeedleDropChange}
        />
        <MobileCheckRow
          title="Disable analog noise effects"
          description="Keep Vinyl ordering and controls without the analog layer."
          checked={vinylAnalogFxDisabled}
          disabled={!enabled}
          onChange={onVinylAnalogFxDisabledChange}
        />
        <label style={{ ...S.rangeRow, ...(!enabled || vinylAnalogFxDisabled ? S.disabled : null) }}>
          <span style={S.rangeHeader}>
            <span style={S.controlTitle}>Needle-drop intensity</span>
            <span style={S.rangeValue}>{Math.round(vinylNeedleDropIntensity * 100)}%</span>
          </span>
          <input
            type="range"
            aria-label="Needle-drop intensity"
            min={0}
            max={100}
            step={1}
            value={Math.round(vinylNeedleDropIntensity * 100)}
            disabled={!enabled || vinylAnalogFxDisabled}
            style={S.range}
            onChange={(event) => onVinylNeedleDropIntensityChange(
              Math.max(0, Math.min(1, Number(event.currentTarget.value) / 100)),
            )}
          />
        </label>
      </div>
    </MobileBottomSheet>
  );
}

function MobileCheckRow({
  title,
  description,
  checked,
  disabled,
  onChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (enabled: boolean) => void;
}) {
  return (
    <label style={{ ...S.toggleRow, ...(disabled ? S.disabled : null) }}>
      <span>
        <span style={S.controlTitle}>{title}</span>
        <span style={S.controlMeta}>{description}</span>
      </span>
      <input
        type="checkbox"
        aria-label={title}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
    </label>
  );
}

const S: Record<string, React.CSSProperties> = {
  intro: {
    display: 'grid',
    gap: 12,
    marginBottom: 16,
    padding: 14,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  sectionTitle: {
    color: 'var(--text)',
    fontSize: 17,
    fontWeight: 800,
    letterSpacing: -0.2,
  },
  description: {
    marginTop: 4,
    color: 'var(--text-muted)',
    fontSize: 13,
    lineHeight: 1.5,
  },
  toggleRow: {
    minHeight: 56,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    padding: '8px 0',
    color: 'var(--text)',
    cursor: 'pointer',
  },
  controlTitle: {
    display: 'block',
    color: 'var(--text)',
    fontSize: 14,
    fontWeight: 750,
  },
  controlMeta: {
    display: 'block',
    marginTop: 3,
    color: 'var(--text-muted)',
    fontSize: 12,
    lineHeight: 1.4,
  },
  vinylHero: {
    minHeight: 160,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 18,
    marginBottom: 16,
    padding: 12,
    border: '1px solid var(--divider-subtle)',
    borderRadius: 16,
    background: 'linear-gradient(145deg, var(--surface-subtle), var(--surface))',
  },
  vinylHeroCopy: {
    minWidth: 0,
    maxWidth: 150,
  },
  modePicker: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gap: 6,
    marginBottom: 12,
    padding: 4,
    borderRadius: 14,
    background: 'var(--surface-subtle)',
  },
  modeButton: {
    minHeight: 44,
    border: 'none',
    borderRadius: 11,
    backgroundColor: 'transparent',
    color: 'var(--text-muted)',
    fontFamily: 'inherit',
    fontSize: 14,
    fontWeight: 750,
  },
  modeButtonActive: {
    backgroundColor: 'var(--accent)',
    color: 'var(--on-accent)',
  },
  controlList: {
    display: 'grid',
    gap: 2,
  },
  rangeRow: {
    minHeight: 68,
    display: 'grid',
    alignContent: 'center',
    gap: 8,
    padding: '8px 0',
  },
  rangeHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  rangeValue: {
    color: 'var(--accent)',
    fontSize: 13,
    fontWeight: 800,
  },
  range: {
    width: '100%',
    minHeight: 44,
    accentColor: 'var(--accent)',
  },
  disabled: {
    opacity: 0.45,
    cursor: 'default',
  },
};
