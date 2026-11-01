import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, setStreamDirect } from '../../api';
import { DEFAULT_SETTINGS } from '../../types';
import MobileSettingsView from './MobileSettingsView';

describe('MobileSettingsView', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    setStreamDirect(false);
    vi.spyOn(api.settings, 'get').mockResolvedValue({
      crossfadeMode: 'auto',
      transcodeQuality: 'high',
    });
    vi.spyOn(api.settings, 'update').mockResolvedValue({ ok: true });
    vi.spyOn(api.dlna, 'status').mockResolvedValue({
      running: true, port: 8200, friendlyName: 'BoogieBox',
    });
    vi.spyOn(api.auth, 'logout').mockResolvedValue({ ok: true });
  });

  it('loads status and updates every mobile preference', async () => {
    render(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText('Running on 8200')).toBeInTheDocument();
    expect(screen.getByText('listener')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('switch', { name: 'Stream Direct' }));
    fireEvent.click(screen.getByRole('switch', { name: 'Crossfade' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Transcode quality' }), { target: { value: 'lossless' } });

    await waitFor(() => {
      expect(api.settings.update).toHaveBeenCalledWith({ crossfadeMode: 'off' });
      expect(api.settings.update).toHaveBeenCalledWith({ transcodeQuality: 'lossless' });
    });
  });

  it('changes Hybrid theme mode, Adaptive accent, and editable Custom colors', async () => {
    const onAppSettingsChange = vi.fn();
    const onHybridThemeModeChange = vi.fn();
    const onAdaptiveAccentEnabledChange = vi.fn();
    render(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
        appSettings={DEFAULT_SETTINGS}
        onAppSettingsChange={onAppSettingsChange}
        hybridThemeMode="custom"
        onHybridThemeModeChange={onHybridThemeModeChange}
        adaptiveAccentEnabled
        onAdaptiveAccentEnabledChange={onAdaptiveAccentEnabledChange}
      />,
    );
    await screen.findByText('Running on 8200');

    expect(screen.getByRole('button', { name: 'Use custom theme' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Use light theme' })).toHaveStyle({ minHeight: '44px' });
    fireEvent.click(screen.getByRole('button', { name: 'Use light theme' }));
    fireEvent.click(screen.getByRole('switch', { name: 'Adaptive accent' }));
    fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#123456' } });

    expect(onHybridThemeModeChange).toHaveBeenCalledWith('light');
    expect(onHybridThemeModeChange).toHaveBeenCalledWith('custom');
    expect(onAdaptiveAccentEnabledChange).toHaveBeenCalledWith(false);
    expect(onAppSettingsChange).toHaveBeenCalledWith(expect.objectContaining({ colorAccent: '#123456' }));
  });

  it('shows settings errors and unavailable DLNA state', async () => {
    vi.mocked(api.settings.get).mockRejectedValue(new Error('settings offline'));
    vi.mocked(api.dlna.status).mockRejectedValue(new Error('dlna offline'));
    render(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText('settings offline')).toBeInTheDocument();
    expect(screen.getByText('Status unavailable')).toBeInTheDocument();
  });

  it('enables initially disabled preferences and renders stopped DLNA without a port', async () => {
    vi.mocked(api.settings.get).mockResolvedValue({
      crossfadeMode: 'off',
      transcodeQuality: '',
    });
    vi.mocked(api.settings.update).mockRejectedValue(new Error('ignored'));
    vi.mocked(api.dlna.status).mockResolvedValue({ running: false, port: null, friendlyName: null });
    render(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText('Stopped')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Transcode quality' })).toHaveValue('low');
    fireEvent.click(screen.getByRole('switch', { name: 'Crossfade' }));
    await waitFor(() => {
      expect(api.settings.update).toHaveBeenCalledWith({ crossfadeMode: 'auto' });
    });
  });

  it('changes volume levelling mode and steps the pre-amp within its range', async () => {
    const onLoudnessChange = vi.fn();
    const { rerender } = render(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
        loudness={{ mode: 'auto', preampDb: 1.5 }}
        onLoudnessChange={onLoudnessChange}
      />,
    );
    await screen.findByText('Running on 8200');
    expect(screen.queryByRole('switch', { name: 'Replay Gain' })).not.toBeInTheDocument();
    const modes = screen.getByRole('group', { name: 'Volume levelling mode' });
    expect(modes.querySelector('[aria-pressed="true"]')).toHaveTextContent('Auto');
    expect(screen.getByText('Album levels when you play an album in order, track levels when you shuffle.')).toBeInTheDocument();
    expect(screen.getByText('+1.5 dB')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Track' }));
    fireEvent.click(screen.getByRole('button', { name: 'Raise pre-amp' }));
    fireEvent.click(screen.getByRole('button', { name: 'Lower pre-amp' }));
    expect(onLoudnessChange).toHaveBeenCalledWith({ mode: 'track' });
    expect(onLoudnessChange).toHaveBeenCalledWith({ preampDb: 2 });
    expect(onLoudnessChange).toHaveBeenCalledWith({ preampDb: 1 });

    rerender(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
        loudness={{ mode: 'off', preampDb: 6 }}
        onLoudnessChange={onLoudnessChange}
      />,
    );
    expect(screen.getByRole('button', { name: 'Raise pre-amp' })).toBeDisabled();
    expect(screen.getByText('Tracks play at their own volume.')).toBeInTheDocument();
  });

  it('shows server operations only to administrators', async () => {
    const { rerender } = render(
      <MobileSettingsView
        currentUser={{ id: 'user-1', username: 'listener', role: 'user' } as any}
        onClose={vi.fn()}
      />,
    );
    await screen.findByText('Running on 8200');
    expect(screen.queryByText('Server operations')).not.toBeInTheDocument();

    rerender(
      <MobileSettingsView
        currentUser={{ id: 'admin-1', username: 'admin', role: 'admin' } as any}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText('Server operations')).toBeInTheDocument();
  });
});
