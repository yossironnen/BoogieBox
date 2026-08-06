/**
 * Tests Mobile App.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import MobileApp from './MobileApp';
import type { MobileSharedProps } from './mobileShell';

vi.mock('../components/Player', () => ({
  default: () => <div data-testid="headless-player">player</div>,
}));

vi.mock('./views/MobileBrowseView', () => ({
  default: ({ onPlayTrack, onAddToQueue, onSelectionChange }: any) => (
    <div>
      browse-view
      <button onClick={() => onPlayTrack({ id: 'browse-track' }, [])}>browse-play</button>
      <button onClick={() => onAddToQueue({ id: 'browse-track' })}>browse-queue</button>
      <button onClick={() => onSelectionChange({ artist: null, album: null, tracks: [] })}>browse-select</button>
    </div>
  ),
}));

vi.mock('./views/MobileSearchView', () => ({
  default: ({ onPlayTrack, onAddToQueue }: any) => (
    <div>
      search-view
      <button onClick={() => onPlayTrack({ id: 'search-track' }, [])}>search-play</button>
      <button onClick={() => onAddToQueue({ id: 'search-track' })}>search-queue</button>
    </div>
  ),
}));

vi.mock('./views/MobilePlaylistsView', () => ({
  default: ({ initialPlaylistId, onSelectionChange, onPlayTrack, onAddToQueue }: any) => (
    <div>
      playlists-view-{initialPlaylistId}
      <button onClick={() => onSelectionChange({ playlist: { id: 'p1', remember_progress: true }, tracks: [] })}>playlist-select</button>
      <button onClick={() => onPlayTrack({ id: 'playlist-track' }, [])}>playlist-play</button>
      <button onClick={() => onAddToQueue({ id: 'playlist-track' })}>playlist-queue</button>
    </div>
  ),
}));

vi.mock('./views/MobileNowPlayingView', () => ({
  default: () => <div>Up Next</div>,
}));

vi.mock('./views/MobileHomeView', () => ({
  default: ({ onOpenAlbum, onOpenPlaylist, onOpenBrowse, onPlayTrack }: any) => (
    <div>
      home-view
      <button onClick={() => onOpenAlbum({ id: 'a1', title: 'Album' })}>home-album</button>
      <button onClick={() => onOpenPlaylist('p9')}>home-playlist</button>
      <button onClick={onOpenBrowse}>home-browse</button>
      <button onClick={() => onPlayTrack({ id: 'home-track' }, [])}>home-play</button>
    </div>
  ),
}));

const { setTrackRating } = vi.hoisted(() => ({ setTrackRating: vi.fn().mockResolvedValue({ ok: true }) }));
vi.mock('../api', () => ({ api: { setTrackRating, albumArtUrl: (id: string) => `/art/${id}` } }));

function createProps(overrides: Partial<MobileSharedProps> = {}): MobileSharedProps {
  return {
    currentUser: { id: '1', username: 'mobile-user', role: 'admin', canManageLibraries: true, canEditMetadata: true },
    libraries: [],
    settings: {
      colorBg: '#000000',
      colorSurface: '#111111',
      colorBorder: '#222222',
      colorAccent: '#ff5500',
      colorText: '#ffffff',
      colorTextMuted: '#999999',
      bgTexture: 'none',
      fontFamily: 'IBM Plex Mono',
      lastfmKey: '',
      dlnaEnabled: 'false',
      dlnaFriendlyName: 'BoogieBox',
      dlnaPort: '8200',
      crossfadeMode: 'off',
      crossfadeDuration: '2',
      vinylMode: 'standard',
      transcodeQuality: 'low',
      replayGainEnabled: 'false',
      lastfmConfigured: 'false',
      waveformGenerateOnMissing: 'true',
      waveformBackgroundEnabled: 'false',
      waveformBackgroundFrequencyHours: '24',
      waveformBackgroundBatchSize: '100',
      bpmBackgroundEnabled: 'false',
      bpmBackgroundFrequencyHours: '24',
      scanDebugLoggingEnabled: 'false',
      deepmixDebugLoggingEnabled: 'false',
      boogiemixOutputFolder: '',
      boogiemixDeepAnalysisBackgroundMode: 'off',
      boogiemixDeepAnalysisPauseBackground: 'false',
      boogiemixDeepAnalysisMaxDurationMins: '15',
      boogiemixDeepAnalysisModel: 'mdx_extra_q',
    },
    hybridThemeMode: 'dark',
    adaptiveAccentEnabled: true,
    hideCompilationOnlyArtists: true,
    ffmpegAvailable: true,
    playbackMode: 'standard',
    vinylHardcore: false,
    vinylNeedleDrop: false,
    vinylAnalogFxDisabled: false,
    vinylNeedleDropIntensity: 0.65,
    playerState: {
      queue: [{ id: '1', file_path: 'x', file_name: 'song.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Song', artist: 'Artist', album: 'Album', library_name: 'Main', track_number: 1, disc_number: 1, year: 2025, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', album_id: '9' }],
      currentIndex: 0,
      isPlaying: true,
      playToken: 1,
    },
    playbackSnapshot: {
      currentTrack: { id: '1', file_path: 'x', file_name: 'song.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100, channels: 2, title: 'Song', artist: 'Artist', album: 'Album', library_name: 'Main', track_number: 1, disc_number: 1, year: 2025, genre: 'Rock', composer: null, comment: null, bpm: null, scanned_at: '2026-01-01', album_id: '9' },
      currentTime: 30,
      duration: 120,
      isPlaying: true,
      volume: 0.5,
      muted: false,
      loading: false,
      audioError: null,
    },
    openPlaylistId: null,
    onPlaybackStateChange: vi.fn(),
    onPlayTrack: vi.fn(),
    onAddToQueue: vi.fn(),
    onConsumeOpenPlaylist: vi.fn(),
    onSettingsChange: vi.fn(),
    onHybridThemeModeChange: vi.fn(),
    onAdaptiveAccentEnabledChange: vi.fn(),
    onHideCompilationOnlyArtistsChange: vi.fn(),
    onPlaybackModeChange: vi.fn(),
    onVinylHardcoreChange: vi.fn(),
    onVinylNeedleDropChange: vi.fn(),
    onVinylAnalogFxDisabledChange: vi.fn(),
    onVinylNeedleDropIntensityChange: vi.fn(),
    ...overrides,
  };
}

describe('MobileApp', () => {
  it('switches tabs and opens now playing from the mini player', () => {
    render(<MobileApp {...createProps()} />);
    expect(screen.getByRole('banner')).toBeInTheDocument();
    expect(screen.getByLabelText('Signed in as mobile-user')).toHaveTextContent('Listening');
    expect(screen.getByRole('navigation', { name: 'Mobile tabs' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Open now playing for/i }));
    expect(screen.getByText('Up Next')).toBeInTheDocument();
    expect(screen.queryByRole('banner')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Open now playing for/i })).not.toBeInTheDocument();
  });

  it('routes every tab and forwards child playback and selection actions', () => {
    const props = createProps();
    render(<MobileApp {...props} />);
    fireEvent.click(screen.getByRole('button', { name: 'home-play' }));
    expect(props.onPlayTrack).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'home-album' }));
    expect(screen.getByText('browse-view')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'browse-play' }));
    fireEvent.click(screen.getByRole('button', { name: 'browse-queue' }));
    fireEvent.click(screen.getByRole('button', { name: 'browse-select' }));

    fireEvent.click(screen.getByRole('button', { name: /Search/ }));
    fireEvent.click(screen.getByRole('button', { name: 'search-play' }));
    fireEvent.click(screen.getByRole('button', { name: 'search-queue' }));

    fireEvent.click(screen.getByRole('button', { name: /Home/ }));
    fireEvent.click(screen.getByRole('button', { name: 'home-playlist' }));
    expect(screen.getByText('playlists-view-p9')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'playlist-select' }));
    fireEvent.click(screen.getByRole('button', { name: 'playlist-play' }));
    fireEvent.click(screen.getByRole('button', { name: 'playlist-queue' }));

    fireEvent.click(screen.getByRole('button', { name: /Now/ }));
    expect(screen.getByText('Up Next')).toBeInTheDocument();
    expect(props.onAddToQueue).toHaveBeenCalledTimes(3);
  });

  it('consumes externally requested playlists and quick-rates the active track', () => {
    const props = createProps({ openPlaylistId: 'external' });
    render(<MobileApp {...props} />);
    expect(screen.getByText('playlists-view-external')).toBeInTheDocument();
    expect(props.onConsumeOpenPlaylist).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /Quick rate/i }));
    expect(setTrackRating).toHaveBeenCalledWith('1', 4);
  });
});

