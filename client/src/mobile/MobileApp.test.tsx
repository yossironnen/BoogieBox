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
  default: () => <div>browse-view</div>,
}));

vi.mock('./views/MobileSearchView', () => ({
  default: () => <div>search-view</div>,
}));

vi.mock('./views/MobilePlaylistsView', () => ({
  default: () => <div>playlists-view</div>,
}));

vi.mock('./views/MobileNowPlayingView', () => ({
  default: () => <div>Up Next</div>,
}));

function createProps(overrides: Partial<MobileSharedProps> = {}): MobileSharedProps {
  return {
    currentUser: { id: '1', username: 'mobile-user', role: 'admin', canScan: true, canEditMetadata: true },
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
      boogiemixOutputFolder: '',
      boogiemixDeepAnalysisBackgroundMode: 'off',
      boogiemixDeepAnalysisPauseBackground: 'false',
      boogiemixDeepAnalysisMaxDurationMins: '15',
    },
    ffmpegAvailable: true,
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
    ...overrides,
  };
}

describe('MobileApp', () => {
  it('switches tabs and opens now playing from the mini player', () => {
    render(<MobileApp {...createProps()} />);
    fireEvent.click(screen.getByRole('button', { name: /Open now playing for/i }));
    expect(screen.getByText('Up Next')).toBeInTheDocument();
  });
});

