import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { NativeAudioSettings, NativeDevice } from './nativeTypes';

const fake = vi.hoisted(() => ({
  settings: { deviceId: null, mode: 'exclusive', bufferMs: 50, nearestRate: true } as NativeAudioSettings,
  saved: [] as NativeAudioSettings[],
  devices: [
    { id: 'fiio', name: 'FiiO Q series', isDefault: true, maxRate: 384_000, maxBits: 32 },
    { id: 'hdmi', name: 'Dell AW3821DW', isDefault: false, maxRate: 48_000, maxBits: 16, singleRate: true },
  ] as NativeDevice[],
}));

vi.mock('../platform', () => ({
  platform: {
    isDesktop: true,
    audio: {
      command: vi.fn(async () => {}),
      devices: vi.fn(async () => fake.devices),
      settings: vi.fn(async () => fake.settings),
      saveSettings: vi.fn(async (s: NativeAudioSettings) => { fake.saved.push(s); }),
      machineName: vi.fn(async () => 'STUDIO-PC'),
      onEvent: vi.fn(async () => () => {}),
    },
  },
  nativeAudioAvailable: async () => true,
}));

import AudioOutputSettings from './AudioOutputSettings';
import PureModeNote from './PureModeNote';
import { resetNativeAudioStore } from './useNativeAudio';
import { OutputPickerList } from '../remote/OutputPicker';
import type { PlayOnApi } from '../remote/usePlayOn';
import type { PlaybackTarget } from '../remote/remoteTypes';

function playOn(target: PlaybackTarget, selectTarget = vi.fn()): PlayOnApi {
  return {
    available: true,
    target,
    renderers: [],
    renderersLoaded: true,
    discovering: false,
    watchRenderers: () => () => {},
    refreshRenderers: () => {},
    selectTarget,
    pendingUdn: null,
    deviceName: null,
    remoteView: null,
    remoteActions: {} as PlayOnApi['remoteActions'],
    playerState: { queue: [], currentIndex: 0, isPlaying: false, playToken: 0 },
    dispatch: () => {},
    notice: null,
    notify: () => {},
    takeover: null,
    openSettings: null,
    openAudioSettings: null,
  } as PlayOnApi;
}

describe('desktop Hi-Res output UI', () => {
  beforeEach(() => {
    fake.saved = [];
    fake.settings = { deviceId: null, mode: 'exclusive', bufferMs: 50, nearestRate: true };
    resetNativeAudioStore();
  });

  it('offers "This PC (<name>)" first and picks it', async () => {
    const select = vi.fn();
    render(<OutputPickerList playOn={playOn({ kind: 'local' }, select)} />);
    const row = await screen.findByRole('option', { name: /This PC \(STUDIO-PC\)/ });
    expect(row).toHaveTextContent('FiiO Q series · Exclusive');
    expect(screen.getAllByRole('option')[0]).toBe(row);
    expect(screen.queryByRole('option', { name: /This browser/ })).toBeNull();
    expect(screen.queryByRole('group', { name: 'Output device' })).toBeNull();
    fireEvent.click(row);
    expect(select).toHaveBeenCalledWith({ kind: 'native' });
  });

  it('lists the devices under the chosen row and saves a pick', async () => {
    const open = vi.fn();
    render(<OutputPickerList playOn={playOn({ kind: 'native' })} onOpenAudioSettings={open} />);
    const group = await screen.findByRole('group', { name: 'Output device' });
    expect(group).toHaveTextContent('up to 384 kHz');
    expect(group).toHaveTextContent('48 kHz only');
    expect(screen.getByRole('radio', { name: /FiiO Q series/ })).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(screen.getByRole('radio', { name: /Dell AW3821DW/ }));
    expect(fake.saved[fake.saved.length - 1]).toMatchObject({ deviceId: 'hdmi' });
    fireEvent.click(screen.getByRole('button', { name: 'Audio output settings' }));
    expect(open).toHaveBeenCalled();
  });

  it('keeps device rows inside the picker: long names shorten, the rate stays whole', async () => {
    render(<OutputPickerList playOn={playOn({ kind: 'native' })} />);
    const group = await screen.findByRole('group', { name: 'Output device' });
    expect(group.style.gridTemplateColumns).toBe('minmax(0, 1fr)');
    const row = screen.getByRole('radio', { name: /FiiO Q series/ });
    expect(row.style.minWidth).toBe('0px');
    const rate = within(row).getByText('up to 384 kHz');
    expect(rate.style.flexShrink).toBe('0');
  });

  it('saves every Audio output setting', async () => {
    render(<AudioOutputSettings />);
    await screen.findByText('Audio output');
    expect(await screen.findByText('How this PC plays when "This PC (STUDIO-PC)" is the output.')).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: /System default/ })).toHaveTextContent('Follows Windows (now FiiO Q series)');
    expect(screen.getByRole('radio', { name: /^FiiO Q series/ })).toHaveTextContent('Up to 384 kHz · 32-bit');
    expect(screen.getByRole('radio', { name: /Dell AW3821DW/ })).toHaveTextContent('48 kHz · 16-bit only');
    fireEvent.click(screen.getByRole('radio', { name: /^Shared/ }));
    expect(fake.saved[fake.saved.length - 1]).toMatchObject({ mode: 'shared' });
    fireEvent.click(screen.getByRole('radio', { name: 'Safe · 150 ms' }));
    expect(fake.saved[fake.saved.length - 1]).toMatchObject({ bufferMs: 150 });
    fireEvent.click(screen.getByRole('switch', { name: 'Use the nearest rate the DAC can play' }));
    await waitFor(() => expect(fake.saved[fake.saved.length - 1]).toMatchObject({ nearestRate: false }));
    expect(screen.getByText('Saved on this PC, not per user.')).toBeInTheDocument();
  });

  it('turns Pure mode off and on, and says what it turns off', async () => {
    render(<><AudioOutputSettings /><PureModeNote /></>);
    const pure = await screen.findByRole('switch', { name: 'Pure mode' });
    expect(pure).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByText('Turns off EQ, volume levelling and crossfade, so nothing changes the sound. On by default in Exclusive mode.')).toBeInTheDocument();
    expect(screen.getByText('Off for This PC while Pure mode is on.')).toBeInTheDocument();
    fireEvent.click(pure);
    await waitFor(() => expect(fake.saved[fake.saved.length - 1]).toMatchObject({ pure: false }));
    expect(screen.queryByText('Off for This PC while Pure mode is on.')).toBeNull();
  });
});
