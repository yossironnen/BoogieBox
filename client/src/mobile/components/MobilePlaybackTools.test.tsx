/**
 * Tests the touch-safe mobile Equalizer and Vinyl control sheets.
 */

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import {
  BUILTIN_PARAMETRIC_PRESETS,
  DEFAULT_PARAMETRIC_BANDS,
} from '../../audio/eq';
import type { PlaybackSnapshot, PlayerEqControls } from '../../components/Player';
import {
  MobileEqualizerSheet,
  MobileVinylSheet,
} from './MobilePlaybackTools';

vi.mock('../../api', () => ({
  api: {
    albumArtUrl: (albumId: string) => `/art/${albumId}`,
  },
}));

function eqControls(): PlayerEqControls {
  return {
    bands: DEFAULT_PARAMETRIC_BANDS.map((band) => ({ ...band })),
    profile: 'Manual',
    customProfiles: [],
    autoEqEnabled: false,
    autoEqCurrentPreset: 'Rock',
    onAutoEqEnabledChange: vi.fn(),
    onBandsChange: vi.fn(),
    onProfileChange: vi.fn(),
    onSaveProfile: vi.fn().mockResolvedValue(null),
    onDeleteProfile: vi.fn().mockResolvedValue(undefined),
  };
}

describe('MobilePlaybackTools', () => {
  it('connects the touch-safe Equalizer sheet to the active player controller', () => {
    const controls = eqControls();
    const onClose = vi.fn();
    render(<MobileEqualizerSheet controls={controls} onClose={onClose} />);

    expect(screen.getByRole('dialog', { name: 'Equalizer' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close equalizer' })).toHaveStyle({ height: '44px' });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Auto EQ' }));
    expect(controls.onAutoEqEnabledChange).toHaveBeenCalledWith(true);

    fireEvent.change(screen.getByRole('combobox', { name: 'EQ profile' }), {
      target: { value: 'Warm' },
    });
    expect(controls.onProfileChange).toHaveBeenCalledWith(
      'Warm',
      BUILTIN_PARAMETRIC_PRESETS.Warm,
    );
    expect(screen.getByRole('button', { name: /Band 1/i })).toHaveStyle({ minHeight: '58px' });
  });

  it('shows a useful Equalizer loading state before the player bridge is ready', () => {
    render(<MobileEqualizerSheet controls={null} onClose={vi.fn()} />);
    expect(screen.getByRole('status')).toHaveTextContent('Connecting to the playback equalizer');
  });

  it('controls persisted Vinyl mode, options, and intensity from Now Playing', () => {
    const onPlaybackModeChange = vi.fn();
    const onVinylHardcoreChange = vi.fn();
    const onVinylNeedleDropChange = vi.fn();
    const onVinylAnalogFxDisabledChange = vi.fn();
    const onVinylNeedleDropIntensityChange = vi.fn();
    const snapshot: PlaybackSnapshot = {
      currentTrack: {
        id: '1',
        file_path: 'x',
        file_name: 'song.mp3',
        file_size: 1,
        format: 'MP3',
        duration: 120,
        bitrate: 320,
        sample_rate: 44100,
        channels: 2,
        title: 'Song',
        artist: 'Artist',
        album: 'Album',
        library_name: 'Main',
        track_number: 1,
        disc_number: 1,
        year: 2025,
        genre: 'Rock',
        composer: null,
        comment: null,
        bpm: null,
        scanned_at: '2026-01-01',
        album_id: '9',
      },
      currentTime: 30,
      duration: 120,
      isPlaying: true,
      volume: 0.5,
      muted: false,
      loading: false,
      audioError: null,
    };

    const { rerender } = render(
      <MobileVinylSheet
        snapshot={snapshot}
        playbackMode="standard"
        vinylHardcore={false}
        vinylNeedleDrop={false}
        vinylAnalogFxDisabled={false}
        vinylNeedleDropIntensity={0.65}
        onPlaybackModeChange={onPlaybackModeChange}
        onVinylHardcoreChange={onVinylHardcoreChange}
        onVinylNeedleDropChange={onVinylNeedleDropChange}
        onVinylAnalogFxDisabledChange={onVinylAnalogFxDisabledChange}
        onVinylNeedleDropIntensityChange={onVinylNeedleDropIntensityChange}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole('dialog', { name: 'Vinyl' })).toBeInTheDocument();
    expect(screen.getByRole('group', { name: /Song vinyl turntable/i })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Vinyl' }));
    expect(onPlaybackModeChange).toHaveBeenCalledWith('vinyl');

    rerender(
      <MobileVinylSheet
        snapshot={snapshot}
        playbackMode="vinyl"
        vinylHardcore={false}
        vinylNeedleDrop={false}
        vinylAnalogFxDisabled={false}
        vinylNeedleDropIntensity={0.65}
        onPlaybackModeChange={onPlaybackModeChange}
        onVinylHardcoreChange={onVinylHardcoreChange}
        onVinylNeedleDropChange={onVinylNeedleDropChange}
        onVinylAnalogFxDisabledChange={onVinylAnalogFxDisabledChange}
        onVinylNeedleDropIntensityChange={onVinylNeedleDropIntensityChange}
        onClose={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole('checkbox', { name: 'Hardcore Vinyl' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Needle-drop sound' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Disable analog noise effects' }));
    fireEvent.change(screen.getByRole('slider', { name: 'Needle-drop intensity' }), {
      target: { value: '82' },
    });
    expect(onVinylHardcoreChange).toHaveBeenCalledWith(true);
    expect(onVinylNeedleDropChange).toHaveBeenCalledWith(true);
    expect(onVinylAnalogFxDisabledChange).toHaveBeenCalledWith(true);
    expect(onVinylNeedleDropIntensityChange).toHaveBeenCalledWith(0.82);
  });
});
