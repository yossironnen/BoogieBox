import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, setStreamDirect } from '../api';
import type { Track } from '../types';
import Player, { type PlayerState } from './Player';

const { playNeedleDrop, preloadVinylFx } = vi.hoisted(() => ({
  playNeedleDrop: vi.fn(),
  preloadVinylFx: vi.fn(),
}));

vi.mock('../audio/VinylFxEngine', () => ({ playNeedleDrop, preloadVinylFx }));

function track(id: string, overrides: Partial<Track> = {}): Track {
  return {
    id,
    file_path: `D:\\Music\\track-${id}.mp3`,
    file_name: `track-${id}.mp3`,
    file_size: 1000,
    format: 'mp3',
    duration: 100,
    bitrate: 320,
    sample_rate: 44100,
    channels: 2,
    title: `Track ${id}`,
    artist: 'Artist',
    album: 'Album',
    album_id: 'album-1',
    library_name: 'Music',
    track_number: 1,
    disc_number: 1,
    year: 2026,
    genre: 'Electronic',
    composer: null,
    comment: null,
    bpm: 120,
    scanned_at: '2026-07-24T00:00:00Z',
    ...overrides,
  } as Track;
}

function canvasContext() {
  const gradient = { addColorStop: vi.fn() };
  return {
    arc: vi.fn(),
    arcTo: vi.fn(),
    beginPath: vi.fn(),
    clearRect: vi.fn(),
    closePath: vi.fn(),
    createLinearGradient: vi.fn(() => gradient),
    createRadialGradient: vi.fn(() => gradient),
    fill: vi.fn(),
    fillRect: vi.fn(),
    fillText: vi.fn(),
    getImageData: vi.fn(() => ({ data: new Uint8ClampedArray([74, 158, 255, 255]) })),
    lineTo: vi.fn(),
    moveTo: vi.fn(),
    roundRect: vi.fn(),
    save: vi.fn(),
    scale: vi.fn(),
    restore: vi.fn(),
    stroke: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    font: '',
    lineWidth: 1,
    shadowBlur: 0,
    shadowColor: '',
    shadowOffsetX: 0,
    shadowOffsetY: 0,
    textAlign: '',
    textBaseline: '',
  } as unknown as CanvasRenderingContext2D;
}

function installAudioContext() {
  const analysers: any[] = [];
  const context: any = {
    state: 'suspended',
    destination: {},
    resume: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    createMediaElementSource: vi.fn(() => ({ connect: vi.fn() })),
    createBiquadFilter: vi.fn(() => ({
      type: 'peaking',
      frequency: { value: 0 },
      Q: { value: 0 },
      gain: { value: 0 },
      connect: vi.fn(),
    })),
    createAnalyser: vi.fn(() => {
      const analyser: any = {
        context,
        fftSize: 256,
        smoothingTimeConstant: 0,
        frequencyBinCount: 8,
        connect: vi.fn(),
        disconnect: vi.fn(),
        getByteTimeDomainData: vi.fn((data: Uint8Array) => data.fill(160)),
        getByteFrequencyData: vi.fn((data: Uint8Array) => data.fill(200)),
      };
      analysers.push(analyser);
      return analyser;
    }),
    createChannelMerger: vi.fn(() => ({ connect: vi.fn(), disconnect: vi.fn() })),
  };
  (window as any).AudioContext = class {
    constructor() { return context; }
  };
  return { context, analysers };
}

function setMediaState(media: HTMLMediaElement, values: { currentTime?: number; duration?: number; error?: any }) {
  if (values.currentTime !== undefined) {
    Object.defineProperty(media, 'currentTime', { configurable: true, writable: true, value: values.currentTime });
  }
  if (values.duration !== undefined) {
    Object.defineProperty(media, 'duration', { configurable: true, value: values.duration });
  }
  if (values.error !== undefined) {
    Object.defineProperty(media, 'error', { configurable: true, value: values.error });
  }
}

describe('Player comprehensive behavior', () => {
  let ctx2d: CanvasRenderingContext2D;
  let rafCallbacks: FrameRequestCallback[];

  beforeEach(() => {
    vi.restoreAllMocks();
    playNeedleDrop.mockReset().mockResolvedValue(undefined);
    preloadVinylFx.mockReset().mockResolvedValue(undefined);
    setStreamDirect(false);
    localStorage.clear();
    ctx2d = canvasContext();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx2d);
    vi.spyOn(HTMLMediaElement.prototype, 'load').mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue(undefined);
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: vi.fn(),
    });
    rafCallbacks = [];
    vi.stubGlobal('requestAnimationFrame', vi.fn((cb: FrameRequestCallback) => {
      rafCallbacks.push(cb);
      return rafCallbacks.length;
    }));
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    vi.spyOn(api.userSettings, 'get').mockResolvedValue({});
    vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
    vi.spyOn(api.crossfade, 'config').mockResolvedValue({ mode: 'off', duration: 2, source: 'global' });
    vi.spyOn(api, 'trackWaveform').mockResolvedValue({ status: 'missing', waveform: null } as any);
    vi.spyOn(api, 'trackSonicFingerprint').mockResolvedValue(null);
    vi.spyOn(api.playlists, 'saveTrackProgress').mockResolvedValue({ ok: true });
    vi.spyOn(api, 'trackLyrics').mockResolvedValue({ lyrics: '', source: 'cache', synced: [] });
    vi.spyOn(api, 'trackEqProfile').mockResolvedValue({ eq_profile: 'Rock', source: 'genre' });
  });

  afterEach(() => {
    delete (window as any).AudioContext;
    delete (window as any).webkitAudioContext;
    vi.unstubAllGlobals();
    setStreamDirect(false);
  });

  it('draws and cycles every visualizer mode and wave style using the live analyser graph', async () => {
    const { context } = installAudioContext();
    localStorage.setItem('vizMode', 'bars');
    const state: PlayerState = {
      queue: [track('1')],
      currentIndex: 0,
      isPlaying: true,
      playToken: 1,
    };
    render(<Player state={state} onStateChange={vi.fn()} ffmpegAvailable />);

    await waitFor(() => expect(context.createAnalyser).toHaveBeenCalled());
    expect(ctx2d.fillRect).toHaveBeenCalled();
    fireEvent.click(screen.getByTitle('Switch to needle meter'));
    expect(screen.getByTitle('Switch to HiFi meter')).toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Switch to HiFi meter'));
    expect(screen.getByTitle('Switch to visualizer')).toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Switch to visualizer'));
    expect(screen.getByTitle('Switch to bar meter')).toBeInTheDocument();

    const waveStyle = screen.getByTitle(/Wave style:/);
    fireEvent.click(waveStyle);
    fireEvent.click(screen.getByTitle(/Wave style:/));
    fireEvent.click(screen.getByTitle(/Wave style:/));
    fireEvent.click(screen.getByTitle(/Wave style:/));
    fireEvent.click(screen.getByTitle('Switch to bar meter'));

    expect(ctx2d.arc).toHaveBeenCalled();
    expect(ctx2d.createLinearGradient).toHaveBeenCalled();
    expect(ctx2d.createRadialGradient).toHaveBeenCalled();
    expect(localStorage.getItem('vizMode')).toBe('bars');
    expect(context.resume).toHaveBeenCalled();
  });

  it('covers transport, queue, settings, seek, lyrics, fingerprint, and audio error paths', async () => {
    delete (window as any).AudioContext;
    setStreamDirect(true);
    vi.mocked(api.userSettings.get).mockResolvedValue({
      volume: '0.75',
      muted: 'false',
      autoEqEnabled: 'true',
      parametricEqSelectedProfile: 'Manual',
    });
    vi.mocked(api.trackWaveform).mockResolvedValue({ status: 'missing', waveform: null } as any);
    vi.mocked(api.trackSonicFingerprint).mockResolvedValue({
      trackId: '2',
      bpmDetected: 124,
      energyScoreRefined: 0.8,
      confidence: 0.9,
      sourceDurationSec: 100,
      demucsModel: 'htdemucs',
      usedGpu: true,
      analysisSchemaVersion: 1,
      sectionJson: [],
      vocalWindowsJson: [],
      drumWindowsJson: [],
      bassWindowsJson: [],
      transitionWindowsJson: [],
      introOutroRefinedJson: { introEnd: 10, outroStart: 90 },
      phraseBoundariesJson: [],
    });
    const onStateChange = vi.fn();
    const onSnapshot = vi.fn();
    const state: PlayerState = {
      queue: [track('1'), track('2', { progress_seconds: 20 } as any), track('3')],
      currentIndex: 1,
      isPlaying: false,
      playToken: 2,
      queueSource: { type: 'playlist', id: 'playlist-1', rememberProgress: true } as any,
    };
    const { container } = render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable
        onPlaybackSnapshotChange={onSnapshot}
      />,
    );
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    setMediaState(audioA, { currentTime: 0, duration: 100, error: null });
    fireEvent.canPlay(audioA);
    expect(audioA.currentTime).toBe(20);
    setMediaState(audioA, { currentTime: 30, duration: 100, error: null });
    fireEvent.timeUpdate(audioA);
    await waitFor(() => expect(api.playlists.saveTrackProgress).toHaveBeenCalledWith(
      'playlist-1', '2', 30,
    ));

    expect(await screen.findByTestId('fp-badge-bpm')).toHaveTextContent('124 BPM');
    fireEvent.click(screen.getByTestId('fp-toggle-button'));
    expect(screen.getByLabelText('Hide Sonic Fingerprint')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Hide Sonic Fingerprint'));

    fireEvent.click(screen.getByTitle('Shuffle queue'));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ queue: expect.any(Array) }));
    fireEvent.click(screen.getByLabelText('Repeat off'));
    expect(screen.getByLabelText('Repeat track')).toBeInTheDocument();

    const bar = audioA.parentElement?.querySelector('div[style*="height: 100px"]') ?? container;
    const transport = Array.from(bar.querySelectorAll('button')).filter(button => !button.title).slice(0, 3);
    fireEvent.click(transport[2]);
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ playToken: 3 }));
    setMediaState(audioA, { currentTime: 8 });
    fireEvent.click(transport[0]);
    expect(audioA.currentTime).toBe(0);
    fireEvent.click(transport[0]);
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 0 }));

    const rightControls = screen.getByTestId('player-right-controls');
    const rightButtons = within(rightControls).getAllByRole('button');
    fireEvent.click(rightButtons[3]);
    const sliders = Array.from(rightControls.querySelectorAll('div')).filter(
      element => element.style.cursor === 'pointer',
    );
    const volumeSlider = sliders[sliders.length - 1];
    vi.spyOn(volumeSlider, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 0, left: 0, top: 0, right: 12, bottom: 60, width: 12, height: 60,
      toJSON: () => ({}),
    });
    fireEvent.mouseDown(volumeSlider, { clientY: 30 });
    fireEvent.mouseMove(window, { clientY: 15 });
    fireEvent.mouseUp(window, { clientY: 15 });

    fireEvent.click(rightButtons[rightButtons.length - 1]);
    expect(screen.getByRole('dialog', { name: 'Playback queue' })).toBeInTheDocument();
    expect(screen.getByRole('listbox', { name: 'Queued tracks' })).toBeInTheDocument();
    const thirdRow = screen.getAllByText('Track 3')[0].closest<HTMLElement>('div[style*="align-items: center"]')!;
    fireEvent.click(thirdRow);
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 2 }));
    fireEvent.click(within(thirdRow).getByRole('button'));
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));

    vi.mocked(api.trackLyrics).mockRejectedValueOnce(new Error('Lyrics unavailable'));
    fireEvent.click(screen.getByLabelText('Show lyrics'));
    expect(await screen.findByText('Lyrics unavailable')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Show lyrics'));

    fireEvent.click(screen.getByLabelText('Equalizer'));
    expect(screen.getByRole('dialog', { name: 'Equalizer' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Auto EQ' }));
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('dialog', { name: 'Equalizer' })).not.toBeInTheDocument();

    fireEvent.waiting(audioA);
    fireEvent.canPlay(audioA);
    setMediaState(audioA, { error: { code: 2 } });
    fireEvent.error(audioA);
    expect(await screen.findByText(/retrying with transcoding/)).toBeInTheDocument();
    fireEvent.error(audioA);
    expect(await screen.findByText(/Network error/)).toBeInTheDocument();
    expect(onSnapshot).toHaveBeenCalledWith(expect.objectContaining({ currentTrack: expect.objectContaining({ id: '2' }) }));
  });

  it('performs zero-gap and crossfade handoffs through both audio slots', async () => {
    installAudioContext();
    const baseState: PlayerState = {
      queue: [track('1'), track('2')],
      currentIndex: 0,
      isPlaying: true,
      playToken: 1,
    };
    vi.mocked(api.crossfade.config).mockResolvedValue({ mode: 'zerogap', duration: 2, source: 'global' });
    const zeroGapChange = vi.fn();
    const zero = render(<Player state={baseState} onStateChange={zeroGapChange} ffmpegAvailable />);
    const [zeroA, zeroB] = Array.from(zero.container.querySelectorAll('audio'));
    await waitFor(() => expect(api.crossfade.config).toHaveBeenCalled());
    setMediaState(zeroA, { currentTime: 99.8, duration: 100 });
    fireEvent.timeUpdate(zeroA);
    expect(zeroB.src).toContain('/api/tracks/2/stream');
    fireEvent.ended(zeroA);
    expect(zeroGapChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));
    zero.unmount();

    vi.mocked(api.crossfade.config).mockResolvedValue({ mode: 'crossfade', duration: 2, source: 'global' });
    const crossfadeChange = vi.fn();
    const cross = render(<Player state={baseState} onStateChange={crossfadeChange} ffmpegAvailable />);
    const [crossA, crossB] = Array.from(cross.container.querySelectorAll('audio'));
    await waitFor(() => expect(api.crossfade.config).toHaveBeenLastCalledWith());
    setMediaState(crossA, { currentTime: 99, duration: 100 });
    fireEvent.timeUpdate(crossA);
    const now = vi.spyOn(performance, 'now').mockReturnValueOnce(0).mockReturnValue(2500);
    fireEvent.canPlay(crossB);
    const ramp = rafCallbacks[rafCallbacks.length - 1];
    ramp(2500);
    expect(crossfadeChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 1, isPlaying: true }));
    setMediaState(crossB, { currentTime: 12, duration: 100 });
    fireEvent.waiting(crossB);
    fireEvent.canPlay(crossB);
    fireEvent.timeUpdate(crossB);
    fireEvent.click(screen.getByLabelText('Repeat off'));
    fireEvent.ended(crossB);
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalled();
    fireEvent.click(screen.getByLabelText('Repeat track'));
    setMediaState(crossB, { currentTime: 99, duration: 100 });
    fireEvent.timeUpdate(crossB);
    now.mockRestore();
  });

  it('covers sparse metadata, synced lyrics, repeat wrapping, queue index removal, and fatal media codes', async () => {
    delete (window as any).AudioContext;
    setStreamDirect(true);
    vi.mocked(api.userSettings.get).mockResolvedValue({
      volume: 'invalid',
      muted: 'true',
      autoEqEnabled: 'false',
      parametricEqSelectedProfile: 'Unknown',
      parametricEqBands: 'invalid-json',
      parametricEqCustomProfiles: 'invalid-json',
    });
    vi.mocked(api.trackWaveform).mockResolvedValue({
      status: 'ready',
      waveform: { points: [0, 0.5, 1], duration: 61 },
    } as any);
    vi.mocked(api.trackSonicFingerprint).mockResolvedValue({
      trackId: 'sparse',
      bpmDetected: null,
      energyScoreRefined: 0,
      confidence: 0,
      sourceDurationSec: 0,
      demucsModel: null,
      usedGpu: false,
      analysisSchemaVersion: 1,
      sectionJson: [],
      vocalWindowsJson: [],
      drumWindowsJson: [],
      bassWindowsJson: [],
      transitionWindowsJson: [],
      introOutroRefinedJson: null,
      phraseBoundariesJson: [],
    } as any);
    vi.mocked(api.trackLyrics).mockResolvedValue({
      lyrics: 'First line\nSecond line',
      source: 'lrclib',
      synced: [{ time: 0, text: 'First line' }, { time: 10, text: 'Second line' }],
    } as any);
    const sparse = track('sparse', {
      title: null,
      artist: null,
      album: null,
      album_id: null,
      duration: null,
      bitrate: null,
      year: null,
      genre: null,
      bpm: null,
      file_name: 'mystery.xyz',
      format: null,
    });
    const onStateChange = vi.fn();
    const { container } = render(
      <Player
        state={{ queue: [track('first'), sparse], currentIndex: 1, isPlaying: false, playToken: 5 }}
        onStateChange={onStateChange}
        ffmpegAvailable
      />,
    );
    const [audioA] = Array.from(container.querySelectorAll('audio'));
    setMediaState(audioA, { currentTime: 12, duration: 61, error: null });
    fireEvent.durationChange(audioA);
    fireEvent.timeUpdate(audioA);
    expect(await screen.findByText('mystery.xyz')).toBeInTheDocument();
    expect(screen.getByText('Unknown artist')).toBeInTheDocument();
    expect(await screen.findByTestId('fp-badge-energy')).toHaveTextContent('0%');
    expect(screen.queryByTestId('fp-badge-bpm')).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText('Show lyrics'));
    expect(await screen.findByText(/First line/)).toBeInTheDocument();
    const karaoke = screen.getByRole('checkbox', { name: 'Karaoke' });
    expect(karaoke).toBeEnabled();
    fireEvent.click(karaoke);
    fireEvent.timeUpdate(audioA);
    expect(screen.getByText('Second line')).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText('Repeat off'));
    fireEvent.click(screen.getByLabelText('Repeat track'));
    const playerBar = audioA.parentElement?.querySelector('div[style*="height: 100px"]') ?? container;
    const transport = Array.from(playerBar.querySelectorAll('button')).filter(button => !button.title).slice(0, 3);
    fireEvent.click(transport[2]);
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 0, isPlaying: true }));
    fireEvent.click(transport[0]);
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 0 }));

    const rightButtons = within(screen.getByTestId('player-right-controls')).getAllByRole('button');
    fireEvent.click(rightButtons[rightButtons.length - 1]);
    const queueRows = screen.getAllByText(/Track first|mystery\.xyz/).map(
      node => node.closest<HTMLElement>('div[style*="align-items: center"]')!,
    );
    fireEvent.click(within(queueRows[0]).getByRole('button'));
    fireEvent.click(within(queueRows[1]).getByRole('button'));

    for (const code of [1, 3, 4, 0]) {
      setMediaState(audioA, { error: { code } });
      fireEvent.error(audioA);
    }
    expect(await screen.findByText(/Playback error/)).toBeInTheDocument();
  });

  it('enforces vinyl locks and triggers configured needle-drop effects', async () => {
    installAudioContext();
    const onStateChange = vi.fn();
    const state: PlayerState = {
      queue: [track('1'), track('2')],
      currentIndex: 0,
      isPlaying: true,
      playToken: 4,
    };
    render(
      <Player
        state={state}
        onStateChange={onStateChange}
        ffmpegAvailable
        playbackMode="vinyl"
        vinylHardcore
        vinylNeedleDrop
        vinylNeedleDropIntensity={0.7}
      />,
    );
    await waitFor(() => expect(preloadVinylFx).toHaveBeenCalled());
    expect(playNeedleDrop).toHaveBeenCalled();
    expect(screen.getByTitle('Shuffle queue')).toBeDisabled();
    expect(screen.getByText(/Hardcore: seeking/)).toBeInTheDocument();

    const rightButtons = within(screen.getByTestId('player-right-controls')).getAllByRole('button');
    fireEvent.click(rightButtons[rightButtons.length - 1]);
    expect(screen.getByRole('button', { name: 'Clear' })).toBeDisabled();
    const queueRow = screen.getAllByText('Track 2')[0].closest<HTMLElement>('div[style*="align-items: center"]')!;
    fireEvent.click(queueRow);
    expect(onStateChange).not.toHaveBeenCalledWith(expect.objectContaining({ currentIndex: 1 }));
    expect(within(queueRow).getByRole('button')).toBeDisabled();
  });
});
