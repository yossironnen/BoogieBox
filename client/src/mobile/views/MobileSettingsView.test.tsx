import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, setStreamDirect } from '../../api';
import MobileSettingsView from './MobileSettingsView';

describe('MobileSettingsView', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    setStreamDirect(false);
    vi.spyOn(api.settings, 'get').mockResolvedValue({
      replayGainEnabled: 'true',
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

    const boxes = screen.getAllByRole('checkbox');
    fireEvent.click(boxes[0]);
    fireEvent.click(boxes[1]);
    fireEvent.click(boxes[2]);
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'lossless' } });

    await waitFor(() => {
      expect(api.settings.update).toHaveBeenCalledWith({ replayGainEnabled: 'false' });
      expect(api.settings.update).toHaveBeenCalledWith({ crossfadeMode: 'off' });
      expect(api.settings.update).toHaveBeenCalledWith({ transcodeQuality: 'lossless' });
    });
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
      replayGainEnabled: 'false',
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
    expect(screen.getByRole('combobox')).toHaveValue('low');
    const boxes = screen.getAllByRole('checkbox');
    fireEvent.click(boxes[1]);
    fireEvent.click(boxes[2]);
    await waitFor(() => {
      expect(api.settings.update).toHaveBeenCalledWith({ replayGainEnabled: 'true' });
      expect(api.settings.update).toHaveBeenCalledWith({ crossfadeMode: 'auto' });
    });
  });
});
