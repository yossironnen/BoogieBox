/**
 * Tests App.Test behavior for BoogieBox regressions.
 */

// @vitest-environment jsdom

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App, { HYBRID_THEME_MODE_STORAGE_KEY } from './App';

vi.mock('./api', () => ({
  getStreamDirect: () => false,
  api: {
    auth: {
      me: vi.fn().mockResolvedValue({ id: '1', username: 'admin', role: 'admin', canScan: true, canEditMetadata: true }),
      logout: vi.fn().mockResolvedValue(undefined),
    },
    systemStatus: vi.fn().mockResolvedValue({ setupRequired: false, ffmpegAvailable: true }),
    libraries: {
      list: vi.fn().mockResolvedValue([
        {
          id: 'lib-1',
          path: 'D:/Music',
          primary_path: 'D:/Music',
          name: 'Main Library',
          library_type: 'music',
          added_at: '2026-01-01',
          last_scan: null,
          track_count: 12,
        },
      ]),
    },
    stats: vi.fn().mockResolvedValue({
      total_tracks: 12,
      total_artists: 3,
      total_albums: 4,
      total_libraries: 1,
      total_hours: 1,
      total_gb: 1,
    }),
    userSettings: {
      get: vi.fn().mockResolvedValue({}),
      update: vi.fn().mockResolvedValue(undefined),
    },
    playbackSettings: vi.fn().mockResolvedValue({}),
    markTrackPlayed: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock('./mobile/useMobileShell', () => ({
  useMobileShell: () => false,
}));

vi.mock('./components/Player', () => ({
  default: () => <div data-testid="player" />,
}));

vi.mock('./components/HomeView', () => ({
  default: () => <div>home-view</div>,
}));

vi.mock('./components/BrowseView', () => ({
  default: () => <div>browse-view</div>,
}));

vi.mock('./components/PlaylistsView', () => ({
  default: () => <div>playlists-view</div>,
}));

vi.mock('./components/SettingsPage', () => ({
  default: () => <div>settings-view</div>,
}));

describe('App sidebar', () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.history.replaceState({}, '', '/');
  });

  it('collapses the left menu to icon-only navigation and persists the choice', async () => {
    render(<App />);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Collapse left menu' })).toBeTruthy());
    expect(screen.getByRole('link', { name: 'Support BoogieBox on Ko-fi' })).toHaveAttribute('href', 'https://ko-fi.com/yronnen');

    fireEvent.click(screen.getByRole('button', { name: 'Collapse left menu' }));

    expect(screen.getByRole('button', { name: 'Expand left menu' })).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'Support BoogieBox on Ko-fi' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Browse Music' }).getAttribute('title')).toBe('Browse Music');
    expect(screen.getByRole('button', { name: 'Main Library' }).getAttribute('title')).toBe('Main Library');
    expect(screen.getByRole('button', { name: 'Log out admin' }).getAttribute('title')).toBe('Log out admin');
    expect(screen.queryByText('BoogieBox')).toBeNull();
    expect(screen.queryByText('Browse Music')).toBeNull();
    expect(screen.queryByText('admin')).toBeNull();
    expect(window.localStorage.getItem('boogiebox.sidebar.collapsed.v1')).toBe('true');
  });

  it('uses the approved production Hybrid shell without preview controls', async () => {
    const { container } = render(<App />);

    await screen.findByText('home-view');
    expect(container.querySelector('[data-ui-preview="hybrid"]')).toBeNull();
    expect(container.querySelector('[data-ui-design="hybrid"]')).toHaveAttribute('data-ui-theme', 'dark');
    expect(screen.queryByLabelText('Hybrid preview controls')).toBeNull();
  });

  it('restores the per-user production Hybrid theme mode', async () => {
    window.localStorage.setItem(`${HYBRID_THEME_MODE_STORAGE_KEY}.u1`, 'light');
    const { container } = render(<App />);

    await screen.findByText('home-view');
    await waitFor(() => {
      expect(container.querySelector('[data-ui-design="hybrid"]')).toHaveAttribute('data-ui-theme', 'light');
    });
    expect(document.documentElement.style.getPropertyValue('--bg')).toBe('#f7f5f2');
  });

  it('opens the guarded real Browse preview and switches temporary theme roles', async () => {
    window.history.replaceState({}, '', '/?ui-preview=hybrid&ui-preview-theme=dark');
    const { container } = render(<App />);

    await screen.findByText('browse-view');
    const root = container.querySelector('[data-ui-preview="hybrid"]');
    expect(root).toHaveAttribute('data-ui-preview-theme', 'dark');
    expect(screen.getByRole('button', { name: 'Dark' })).toHaveAttribute('aria-pressed', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Light' }));

    expect(root).toHaveAttribute('data-ui-preview-theme', 'light');
    expect(document.documentElement.style.getPropertyValue('--bg')).toBe('#f7f5f2');
    expect(window.location.search).toContain('ui-preview-theme=light');
    expect(screen.getByRole('link', { name: 'Exit preview' })).toHaveAttribute('href', '/');
  });
});
