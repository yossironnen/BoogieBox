/**
 * Tests the Artist Radio metadata settings card (mode, keyless providers, progress).
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import RadioMetadataSettings, { tagProgressPercent } from './RadioMetadataSettings';
import type { RadioMetadataStatus } from '../types';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    radioMetadataStatus: vi.fn(),
    settings: { update: vi.fn() },
  },
}));
vi.mock('../api', () => ({ api: apiMock }));

const status = (overrides: Partial<RadioMetadataStatus> = {}): RadioMetadataStatus => ({
  mode: 'full',
  lastfmConfigured: true,
  keylessEnabled: true,
  tracksChecked: 1234,
  tracksTotal: 5000,
  artistsTagged: 80,
  artistsTotal: 100,
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  apiMock.radioMetadataStatus.mockResolvedValue(status());
  apiMock.settings.update.mockResolvedValue({ ok: true });
});

afterEach(() => {
  vi.useRealTimers();
});

describe('tagProgressPercent', () => {
  it('rounds, caps at 100, and reports null when there is nothing to measure', () => {
    expect(tagProgressPercent(1234, 5000)).toBe(25);
    expect(tagProgressPercent(6, 5)).toBe(100);
    expect(tagProgressPercent(0, 0)).toBeNull();
    expect(tagProgressPercent(3, -1)).toBeNull();
  });
});

describe('RadioMetadataSettings', () => {
  it('shows the current mode, keyless setting, and progress with icons', async () => {
    render(<RadioMetadataSettings />);
    expect(screen.getByText('Loading status…')).toBeInTheDocument();
    expect(await screen.findByText(/Tracks checked:/)).toHaveTextContent('1,234 of 5,000 (25%)');
    expect(screen.getByText(/Artists with tags:/)).toHaveTextContent('80 of 100 (80%)');
    expect(screen.getByRole('combobox', { name: 'Track tag collection' })).toHaveValue('full');
    expect(screen.getByRole('checkbox', { name: 'Use MusicBrainz and ListenBrainz' })).toBeChecked();
    expect(screen.getByRole('progressbar', { name: 'Track tag collection progress' })).toHaveAttribute('aria-valuenow', '25');
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('saves a new mode and refreshes the status', async () => {
    render(<RadioMetadataSettings />);
    const select = await screen.findByRole('combobox', { name: 'Track tag collection' });
    apiMock.radioMetadataStatus.mockResolvedValue(status({ mode: 'lazy' }));
    fireEvent.change(select, { target: { value: 'lazy' } });
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ radioTrackTagSync: 'lazy' }));
    await waitFor(() => expect(screen.getByRole('combobox', { name: 'Track tag collection' })).toHaveValue('lazy'));
  });

  it('saves the keyless providers toggle in both directions', async () => {
    render(<RadioMetadataSettings />);
    const box = await screen.findByRole('checkbox', { name: 'Use MusicBrainz and ListenBrainz' });
    apiMock.radioMetadataStatus.mockResolvedValue(status({ keylessEnabled: false }));
    fireEvent.click(box);
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenCalledWith({ radioKeylessProviders: 'false' }));
    await waitFor(() => expect(screen.getByRole('checkbox', { name: 'Use MusicBrainz and ListenBrainz' })).not.toBeChecked());
    fireEvent.click(screen.getByRole('checkbox', { name: 'Use MusicBrainz and ListenBrainz' }));
    await waitFor(() => expect(apiMock.settings.update).toHaveBeenLastCalledWith({ radioKeylessProviders: 'true' }));
  });

  it('points to Integrations when no Last.fm key is configured, unless collection is off', async () => {
    apiMock.radioMetadataStatus.mockResolvedValue(status({ lastfmConfigured: false }));
    const { unmount } = render(<RadioMetadataSettings />);
    expect(await screen.findByRole('note')).toHaveTextContent('Last.fm API key');
    unmount();
    apiMock.radioMetadataStatus.mockResolvedValue(status({ lastfmConfigured: false, mode: 'off' }));
    render(<RadioMetadataSettings />);
    await screen.findByText(/Tracks checked:/);
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('omits percentages and the bar for an empty library', async () => {
    apiMock.radioMetadataStatus.mockResolvedValue(status({ tracksChecked: 0, tracksTotal: 0, artistsTagged: 0, artistsTotal: 0 }));
    render(<RadioMetadataSettings />);
    expect(await screen.findByText(/Tracks checked:/)).toHaveTextContent('0 of 0');
    expect(screen.queryByRole('progressbar')).toBeNull();
  });

  it('disables the controls for non-admins and while status is unavailable', async () => {
    apiMock.radioMetadataStatus.mockRejectedValue(new Error('down'));
    const { unmount } = render(<RadioMetadataSettings />);
    expect(await screen.findByText('Status unavailable.')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Track tag collection' })).toBeDisabled();
    unmount();

    apiMock.radioMetadataStatus.mockResolvedValue(status());
    render(<RadioMetadataSettings disabled />);
    await screen.findByText(/Tracks checked:/);
    expect(screen.getByRole('combobox', { name: 'Track tag collection' })).toBeDisabled();
    expect(screen.getByRole('checkbox', { name: 'Use MusicBrainz and ListenBrainz' })).toBeDisabled();
  });

  it('shows the error when saving fails and keeps the last status', async () => {
    render(<RadioMetadataSettings />);
    const select = await screen.findByRole('combobox', { name: 'Track tag collection' });
    apiMock.settings.update.mockRejectedValueOnce(new Error('Setting is invalid'));
    fireEvent.change(select, { target: { value: 'off' } });
    expect(await screen.findByRole('alert')).toHaveTextContent('Setting is invalid');
    expect(screen.getByText(/Tracks checked:/)).toBeInTheDocument();
    apiMock.settings.update.mockRejectedValueOnce('nope');
    fireEvent.change(screen.getByRole('combobox', { name: 'Track tag collection' }), { target: { value: 'lazy' } });
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Could not save.'));
  });

  it('polls for progress while mounted and stops afterwards', async () => {
    vi.useFakeTimers();
    const { unmount } = render(<RadioMetadataSettings />);
    await act(async () => { await Promise.resolve(); });
    expect(apiMock.radioMetadataStatus).toHaveBeenCalledTimes(1);
    await act(async () => { vi.advanceTimersByTime(15000); await Promise.resolve(); });
    expect(apiMock.radioMetadataStatus).toHaveBeenCalledTimes(2);
    unmount();
    await act(async () => { vi.advanceTimersByTime(45000); });
    expect(apiMock.radioMetadataStatus).toHaveBeenCalledTimes(2);
  });
});
