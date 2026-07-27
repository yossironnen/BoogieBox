/**
 * Tests Player.Component.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Player, { resolveDesktopPlayerDockHeight, type PlayerState } from '../components/Player';
import {
  DESKTOP_PLAYER_DOCK_HEIGHT,
  DESKTOP_VINYL_PLAYER_DOCK_HEIGHT,
} from '../hybridPreview';
import { api } from '../api';

describe('Player artist link', () => {
  beforeEach(() => {
    vi.spyOn(HTMLMediaElement.prototype, 'load').mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
    vi.spyOn(api.userSettings, 'get').mockResolvedValue({});
    vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('opens artist page callback when now-playing artist is clicked', () => {
    const onOpenArtist = vi.fn();
    const onStateChange = vi.fn();

    const state: PlayerState = {
      queue: [
        {
          id: '12',
          file_path: 'D:\\Music\\Awake.mp3',
          file_name: 'Awake.mp3',
          file_size: 123,
          format: 'mp3',
          duration: 220,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Awake',
          artist: 'Tycho',
          album: 'Dive',
          library_name: 'Main',
          track_number: 1,
          disc_number: 1,
          year: 2011,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-02-25T00:00:00Z',
        },
      ],
      currentIndex: 0,
      isPlaying: false,
      playToken: 1,
    };

    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable={true}
        onOpenArtist={onOpenArtist}
        hybridPreview
      />
    );

    const playerBar = document.querySelector('[data-hybrid-preview-surface="player"]');
    expect(playerBar).toBeInTheDocument();
    expect(playerBar).toHaveStyle({
      height: `${DESKTOP_PLAYER_DOCK_HEIGHT}px`,
      minHeight: `${DESKTOP_PLAYER_DOCK_HEIGHT}px`,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open artist Tycho' }));
    expect(onOpenArtist).toHaveBeenCalledWith('Tycho');
  });

  it('resolves standard and Vinyl dock heights without a thinner Hybrid branch', () => {
    expect(resolveDesktopPlayerDockHeight(false)).toBe(DESKTOP_PLAYER_DOCK_HEIGHT);
    expect(resolveDesktopPlayerDockHeight(true)).toBe(DESKTOP_VINYL_PLAYER_DOCK_HEIGHT);
  });

  it('opens album page callback when now-playing album is clicked', () => {
    const onOpenAlbum = vi.fn();
    const onStateChange = vi.fn();

    const state: PlayerState = {
      queue: [
        {
          id: '13',
          file_path: 'D:\\Music\\Hours.mp3',
          file_name: 'Hours.mp3',
          file_size: 123,
          format: 'mp3',
          duration: 220,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Hours',
          artist: 'Tycho',
          album: 'Dive',
          library_name: 'Main',
          track_number: 2,
          disc_number: 1,
          year: 2011,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-02-25T00:00:00Z',
        },
      ],
      currentIndex: 0,
      isPlaying: false,
      playToken: 1,
    };

    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable={true}
        onOpenAlbum={onOpenAlbum}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Open album Dive' }));
    expect(onOpenAlbum).toHaveBeenCalledWith('Dive', 'Tycho');
  });

  it('keeps meters and volume controls grouped at the right edge', () => {
    const onStateChange = vi.fn();

    const state: PlayerState = {
      queue: [
        {
          id: '44',
          file_path: 'D:\\Music\\Aurora.mp3',
          file_name: 'Aurora.mp3',
          file_size: 123,
          format: 'mp3',
          duration: 220,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Aurora',
          artist: 'Tycho',
          album: 'Dive',
          library_name: 'Main',
          track_number: 1,
          disc_number: 1,
          year: 2011,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-02-25T00:00:00Z',
        },
      ],
      currentIndex: 0,
      isPlaying: false,
      playToken: 1,
    };

    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable={true}
      />
    );

    expect(screen.getByTestId('player-right-cluster')).toHaveStyle('margin-left: auto');
    expect(screen.getByTestId('player-right-controls')).not.toHaveStyle('margin-left: auto');
    expect(screen.getByTestId('player-progress-area')).toHaveStyle('max-width: 460px');
    expect(screen.getByTestId('player-progress-area')).toHaveStyle('flex: 1 1 0');
  });

  it('opens lyrics popup and renders lyrics text', async () => {
    const onStateChange = vi.fn();
    const trackLyricsMock = vi.spyOn(api, 'trackLyrics').mockResolvedValue({
      lyrics: 'Line one\nLine two',
      source: 'cache',
      synced: [{ time: 0, text: 'Line one' }, { time: 3, text: 'Line two' }],
    });

    const state: PlayerState = {
      queue: [
        {
          id: '77',
          file_path: 'D:\\Music\\Song.mp3',
          file_name: 'Song.mp3',
          file_size: 123,
          format: 'mp3',
          duration: 180,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Song',
          artist: 'Artist',
          album: 'Album',
          library_name: 'Main',
          track_number: 1,
          disc_number: 1,
          year: 2011,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-02-25T00:00:00Z',
        },
      ],
      currentIndex: 0,
      isPlaying: false,
      playToken: 1,
    };

    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable={true}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Show lyrics' }));

    expect(await screen.findByRole('dialog', { name: 'Lyrics popup' })).toBeInTheDocument();
    expect(trackLyricsMock).toHaveBeenCalledWith('77');
    expect(screen.getByRole('checkbox', { name: 'Karaoke' })).not.toBeDisabled();
  });

  it('opens the 7-band parametric equalizer and persists band edits', async () => {
    const updateSettingsMock = vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
    const onStateChange = vi.fn();

    const state: PlayerState = {
      queue: [
        {
          id: '88',
          file_path: 'D:\\Music\\Parametric.mp3',
          file_name: 'Parametric.mp3',
          file_size: 123,
          format: 'mp3',
          duration: 180,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Parametric',
          artist: 'Artist',
          album: 'Album',
          library_name: 'Main',
          track_number: 1,
          disc_number: 1,
          year: 2026,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-05-19T00:00:00Z',
        },
      ],
      currentIndex: 0,
      isPlaying: false,
      playToken: 1,
    };

    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable={true}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Equalizer' }));

    expect(await screen.findByRole('group', { name: 'EQ bands' })).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: 'EQ mode' })).not.toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /Band \d / })).toHaveLength(7);

    fireEvent.change(screen.getByRole('slider', { name: 'Band 1 gain' }), { target: { value: '3' } });

    await waitFor(() => {
      expect(updateSettingsMock).toHaveBeenCalledWith(expect.objectContaining({
        parametricEqBands: expect.stringContaining('"gainDb":3'),
      }));
    });
  });

  it('migrates legacy graphic equalizer settings into parametric settings', async () => {
    vi.spyOn(api.userSettings, 'get').mockResolvedValue({
      eqSelectedProfile: 'Road Trip',
      eqProfiles: JSON.stringify([{ name: 'Road Trip', gains: [0, 2, 4, 6, 8, 10, 12, -2, -4, -6] }]),
    });
    const updateSettingsMock = vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
    const onStateChange = vi.fn();

    const state: PlayerState = {
      queue: [
        {
          id: '89',
          file_path: 'D:\\Music\\Legacy.mp3',
          file_name: 'Legacy.mp3',
          file_size: 123,
          format: 'mp3',
          duration: 180,
          bitrate: 320,
          sample_rate: 44100,
          channels: 2,
          title: 'Legacy',
          artist: 'Artist',
          album: 'Album',
          library_name: 'Main',
          track_number: 1,
          disc_number: 1,
          year: 2026,
          genre: 'Electronic',
          composer: null,
          comment: null,
          bpm: null,
          scanned_at: '2026-05-19T00:00:00Z',
        },
      ],
      currentIndex: 0,
      isPlaying: false,
      playToken: 1,
    };

    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable={true}
      />
    );

    await waitFor(() => {
      expect(updateSettingsMock).toHaveBeenCalledWith(expect.objectContaining({
        parametricEqBands: expect.stringContaining('"gainDb":1'),
        parametricEqProfiles: expect.stringContaining('Road Trip'),
        parametricEqSelectedProfile: 'Road Trip',
        eqMode: 'parametric',
      }));
    });
  });
});
