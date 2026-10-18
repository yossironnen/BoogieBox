import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import SubsonicSettings, { DEFAULT_IGNORED_ARTICLES } from './SubsonicSettings';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    settings: { get: vi.fn(), update: vi.fn() },
    subsonic: { credentials: vi.fn() },
  },
}));

vi.mock('../api', () => ({ api: apiMock }));

describe('SubsonicSettings', () => {
  beforeEach(() => {
    apiMock.settings.get.mockReset().mockResolvedValue({ subsonicApiEnabled: 'false' });
    apiMock.settings.update.mockReset().mockResolvedValue({ ok: true });
    apiMock.subsonic.credentials.mockReset().mockResolvedValue({ serverUrls: ['http://10.0.0.5:3001'] });
  });

  it('turns the API on and saves the transcode limit', async () => {
    render(<SubsonicSettings />);
    const toggle = screen.getByRole('switch', { name: 'Enable Subsonic API' });
    await waitFor(() => expect(apiMock.settings.get).toHaveBeenCalled());
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    expect(await screen.findByText('http://10.0.0.5:3001')).toBeInTheDocument();

    fireEvent.click(toggle);
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ subsonicApiEnabled: 'true' }));
    expect(toggle).toHaveAttribute('aria-checked', 'true');
    expect(await screen.findByText('Saved')).toBeInTheDocument();

    fireEvent.change(screen.getByRole('combobox'), { target: { value: '5' } });
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ subsonicMaxTranscodes: '5' }));
  });

  it('normalizes and saves ignored articles only when they change', async () => {
    render(<SubsonicSettings />);
    const input = screen.getByRole('textbox') as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(DEFAULT_IGNORED_ARTICLES));
    fireEvent.blur(input);
    expect(apiMock.settings.update).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: '  The   Die  ' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.blur(input);
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ subsonicIgnoredArticles: 'The Die' }));
    await waitFor(() => expect(input.value).toBe('The Die'));
  });

  it('reports save errors and tolerates a failed status load', async () => {
    apiMock.settings.get.mockRejectedValue(new Error('offline'));
    apiMock.subsonic.credentials.mockRejectedValue(new Error('offline'));
    apiMock.settings.update.mockRejectedValue(new Error("Setting 'subsonicMaxTranscodes' must be between 1 and 8"));
    render(<SubsonicSettings />);
    fireEvent.change(screen.getByRole('combobox'), { target: { value: '8' } });
    expect(await screen.findByText("Error: Setting 'subsonicMaxTranscodes' must be between 1 and 8")).toBeInTheDocument();
  });
});
