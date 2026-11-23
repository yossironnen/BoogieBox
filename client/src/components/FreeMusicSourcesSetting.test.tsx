/**
 * Tests the Free music sources switch (Settings → Integrations).
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import FreeMusicSourcesSetting from './FreeMusicSourcesSetting';
import type { RadioMetadataStatus } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    radioMetadataStatus: vi.fn(),
    settings: { update: vi.fn() },
  },
}));
vi.mock('../api', () => ({ api: apiMock }));

const LABEL = 'Use MusicBrainz, ListenBrainz, Wikipedia and TheAudioDB';

const status = (overrides: Partial<RadioMetadataStatus> = {}): RadioMetadataStatus => ({
  mode: 'full',
  keylessEnabled: true,
  tracksChecked: 0,
  tracksTotal: 0,
  artistsTagged: 0,
  artistsTotal: 0,
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  apiMock.radioMetadataStatus.mockResolvedValue(status());
  apiMock.settings.update.mockResolvedValue({ ok: true });
});

describe('FreeMusicSourcesSetting', () => {
  it('shows the saved state and saves the switch in both directions', async () => {
    render(<FreeMusicSourcesSetting />);
    await waitFor(() => expect(screen.getByRole('checkbox', { name: LABEL })).toBeEnabled());
    expect(screen.getByRole('checkbox', { name: LABEL })).toBeChecked();
    apiMock.radioMetadataStatus.mockResolvedValue(status({ keylessEnabled: false }));
    fireEvent.click(screen.getByRole('checkbox', { name: LABEL }));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ radioKeylessProviders: 'false' }));
    await waitFor(() => expect(screen.getByRole('checkbox', { name: LABEL })).not.toBeChecked());
    fireEvent.click(screen.getByRole('checkbox', { name: LABEL }));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenLastCalledWith({ radioKeylessProviders: 'true' }));
  });

  it('is disabled for non-admins and while the state is unknown', async () => {
    apiMock.radioMetadataStatus.mockRejectedValue(new Error('down'));
    const { unmount } = render(<FreeMusicSourcesSetting />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Status unavailable.');
    expect(screen.getByRole('checkbox', { name: LABEL })).toBeDisabled();
    unmount();

    apiMock.radioMetadataStatus.mockResolvedValue(status());
    render(<FreeMusicSourcesSetting disabled />);
    await waitFor(() => expect(apiMock.radioMetadataStatus).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('checkbox', { name: LABEL })).toBeDisabled();
  });

  it('shows the error when saving fails', async () => {
    render(<FreeMusicSourcesSetting />);
    await waitFor(() => expect(screen.getByRole('checkbox', { name: LABEL })).toBeEnabled());
    apiMock.settings.update.mockRejectedValueOnce(new Error('Setting is invalid'));
    fireEvent.click(screen.getByRole('checkbox', { name: LABEL }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Setting is invalid');
  });
});
