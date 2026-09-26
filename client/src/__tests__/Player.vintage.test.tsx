/**
 * Tests the player dock under the Vintage Record Shop style: scoped dark dock
 * palette, bakelite volume knob in place of the slider, and page-palette popups.
 */

import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Player, { type PlayerState } from '../components/Player';
import { api } from '../api';
import { VINTAGE_STYLES, VintageStyleContext } from '../vintageThemes';

const STATE: PlayerState = {
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

function renderPlayer(vintage: boolean) {
  const player = <Player state={STATE} onStateChange={vi.fn()} ffmpegAvailable hybridPreview />;
  return render(
    vintage
      ? <VintageStyleContext.Provider value={VINTAGE_STYLES.recordshop}>{player}</VintageStyleContext.Provider>
      : player,
  );
}

describe('Player — Vintage Record Shop dock', () => {
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

  it('scopes the dark dock palette and uses the bakelite volume knob', () => {
    const { container } = renderPlayer(true);

    const dock = container.querySelector('[data-vintage-dock="recordshop"]') as HTMLElement;
    expect(dock).not.toBeNull();
    expect(dock.style.getPropertyValue('--bg')).toBe('#2b2118');
    expect(dock.style.getPropertyValue('--text')).toBe('#f7efdc');

    const knob = screen.getByRole('slider', { name: 'Volume' });
    expect(knob).toHaveAttribute('aria-valuenow', '50');
    fireEvent.keyDown(knob, { key: 'End' });
    expect(knob).toHaveAttribute('aria-valuenow', '100');
    fireEvent.doubleClick(knob);
    expect(knob).toHaveAttribute('aria-valuetext', 'Muted');
  });

  it('restores the page palette on popups rendered inside the dock', () => {
    renderPlayer(true);
    fireEvent.click(screen.getByRole('button', { name: 'Equalizer' }));
    const eq = screen.getByRole('dialog', { name: 'Equalizer' });
    expect(eq.style.getPropertyValue('--bg')).toBe(VINTAGE_STYLES.recordshop.palette.colorBg);
    expect(eq.style.getPropertyValue('--text')).toBe(VINTAGE_STYLES.recordshop.palette.colorText);
  });

  it('keeps the vertical slider and unscoped dock outside Vintage', () => {
    const { container } = renderPlayer(false);
    expect(container.querySelector('[data-vintage-dock]')).toBeNull();
    expect(screen.queryByRole('slider', { name: 'Volume' })).toBeNull();
  });
});
