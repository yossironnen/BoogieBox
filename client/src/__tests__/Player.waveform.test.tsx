/**
 * Tests Player.Waveform.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Player, { type PlayerState } from '../components/Player';
import type { ClientEntityId } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    trackWaveform: vi.fn(),
    trackStreamUrl: vi.fn((id: ClientEntityId) => `/api/tracks/${id}/stream`),
    crossfade: {
      config: vi.fn(async () => ({ mode: 'off', duration: 2, source: 'global' })),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

const BASE_STATE: PlayerState = {
  queue: [
    {
      id: '7',
      file_path: 'D:\\Music\\test.mp3',
      file_name: 'test.mp3',
      file_size: 123,
      format: 'mp3',
      duration: 180,
      bitrate: 320,
      sample_rate: 44100,
      channels: 2,
      title: 'Test',
      artist: 'Artist',
      album: 'Album',
      library_name: 'Main',
      track_number: 1,
      disc_number: 1,
      year: 2024,
      genre: 'Rock',
      composer: null,
      comment: null,
      bpm: null,
      scanned_at: '2026-02-28T00:00:00Z',
    },
  ],
  currentIndex: 0,
  isPlaying: false,
  playToken: 1,
};

describe('Player waveform integration', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(HTMLMediaElement.prototype, 'load').mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders waveform progress UI when waveform data is available', async () => {
    apiMock.trackWaveform.mockResolvedValue({
      status: 'ready',
      waveform: {
        trackId: 7,
        sampleCount: 4,
        durationSeconds: 180,
        points: [0, 64, 128, 255],
        updatedAt: '2026-02-28T00:00:00Z',
      },
    });

    render(<Player state={BASE_STATE} onStateChange={() => {}} ffmpegAvailable={true} />);

    await waitFor(() => expect(apiMock.trackWaveform).toHaveBeenCalledWith('7'));
    expect(await screen.findByTestId('waveform-bar')).toBeInTheDocument();
  });

  it('falls back when waveform data is missing', async () => {
    apiMock.trackWaveform.mockResolvedValue({ status: 'missing' });

    render(<Player state={BASE_STATE} onStateChange={() => {}} ffmpegAvailable={true} />);

    await waitFor(() => expect(apiMock.trackWaveform).toHaveBeenCalledWith('7'));
    expect(screen.queryByTestId('waveform-bar')).toBeNull();
    expect(screen.getByTestId('player-progress-area')).toBeInTheDocument();
  });
});

