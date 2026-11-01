import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import { formatPreamp, ServerLevellingCard, VolumeLevellingCard } from './LoudnessSettings';

describe('formatPreamp', () => {
  it('signs the value and drops the sign at zero', () => {
    expect(formatPreamp(1.5)).toBe('+1.5 dB');
    expect(formatPreamp(0)).toBe('0 dB');
    expect(formatPreamp(-2)).toBe('−2 dB');
  });
});

describe('VolumeLevellingCard', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(api, 'playbackSettings').mockResolvedValue({
      transcodeQuality: 'low', replayGainEnabled: 'false', vinylMode: 'standard', lastfmConfigured: 'false',
      loudnessPlayOnMode: 'album',
    });
  });

  it('shows the mode, its hint and where levelling applies, and reports changes', async () => {
    const onChange = vi.fn();
    render(<VolumeLevellingCard prefs={{ mode: 'album', preampDb: 0 }} onChange={onChange} />);

    const modes = screen.getByRole('group', { name: 'Volume levelling mode' });
    expect(modes.querySelector('[aria-pressed="true"]')).toHaveTextContent('Album');
    expect(screen.getByText('Albums keep their own quiet and loud tracks.')).toBeInTheDocument();
    expect(await screen.findByText('Levelled (set by an admin)')).toBeInTheDocument();
    expect(screen.getByText('Levelled')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Off' }));
    expect(onChange).toHaveBeenCalledWith({ mode: 'off' });
    fireEvent.change(screen.getByLabelText('Pre-amp'), { target: { value: '2.5' } });
    expect(onChange).toHaveBeenCalledWith({ preampDb: 2.5 });
  });

  it('says this player is not levelled when the mode is off', async () => {
    vi.mocked(api.playbackSettings).mockResolvedValue({
      transcodeQuality: 'low', replayGainEnabled: 'false', vinylMode: 'standard', lastfmConfigured: 'false',
    });
    render(<VolumeLevellingCard prefs={{ mode: 'off', preampDb: 0 }} onChange={vi.fn()} />);
    expect(await screen.findByText('Not levelled (set by an admin)')).toBeInTheDocument();
    expect(screen.getAllByText('Not levelled')).toHaveLength(2);
  });
});

describe('ServerLevellingCard', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(api.settings, 'get').mockResolvedValue({
      loudnessPlayOnMode: 'auto',
      loudnessSubsonicTranscodeMode: 'off',
      loudnessAnalysisEnabled: 'true',
    });
    vi.spyOn(api.settings, 'update').mockResolvedValue({ ok: true });
    vi.spyOn(api.admin, 'loudnessStatus').mockResolvedValue({
      tagged: 1200, measured: 34, remaining: 5, failed: 1, enabled: true, running: true,
    });
  });

  it('loads the server modes and counts, and saves each change', async () => {
    render(<ServerLevellingCard />);
    expect(await screen.findByLabelText('From tags: 1200')).toBeInTheDocument();
    expect(screen.getByLabelText('Couldn’t measure: 1')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText('Play On speakers')).toHaveValue('auto'));
    expect(screen.getByLabelText('Subsonic transcodes').querySelectorAll('option')).toHaveLength(3);

    fireEvent.change(screen.getByLabelText('Play On speakers'), { target: { value: 'track' } });
    fireEvent.change(screen.getByLabelText('Subsonic transcodes'), { target: { value: 'album' } });
    fireEvent.click(screen.getByRole('switch', { name: 'Measure tracks without tags' }));

    await waitFor(() => {
      expect(api.settings.update).toHaveBeenCalledWith({ loudnessPlayOnMode: 'track' });
      expect(api.settings.update).toHaveBeenCalledWith({ loudnessSubsonicTranscodeMode: 'album' });
      expect(api.settings.update).toHaveBeenCalledWith({ loudnessAnalysisEnabled: 'false' });
    });
    expect(await screen.findByText('Saved')).toBeInTheDocument();
  });
});
