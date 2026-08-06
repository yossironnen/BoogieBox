/**
 * Tests Login Screen.Test behavior for BoogieBox regressions.
 */

import React from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import LoginScreen from '../components/LoginScreen';

const { apiMock } = vi.hoisted(() => ({
  apiMock: {
    auth: {
      getLoginUsers: vi.fn(),
      login: vi.fn(),
    },
  },
}));

vi.mock('../api', () => ({
  api: apiMock,
}));

describe('LoginScreen', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.auth.getLoginUsers.mockResolvedValue([
      { id: '1', username: 'Admin' },
      { id: '2', username: 'DJ' },
    ]);
  });

  it('logs in immediately when the selected user has no PIN', async () => {
    const onLogin = vi.fn();
    apiMock.auth.login.mockResolvedValueOnce({
      user: { id: '1', username: 'Admin', role: 'admin', canManageLibraries: true, canEditMetadata: true },
    });

    render(<LoginScreen onLogin={onLogin} />);

    fireEvent.click(await screen.findByRole('button', { name: /Admin/i }));

    await waitFor(() => expect(apiMock.auth.login).toHaveBeenCalledWith('1', undefined, false));
    expect(onLogin).toHaveBeenCalledWith({
      id: '1',
      username: 'Admin',
      role: 'admin',
      canManageLibraries: true,
      canEditMetadata: true,
    });
    expect(screen.queryByText(/Enter PIN if this user has one/i)).not.toBeInTheDocument();
  });

  it('falls back to the PIN screen when a PIN is required', async () => {
    const onLogin = vi.fn();
    apiMock.auth.login.mockRejectedValueOnce(new Error('Invalid credentials'));

    render(<LoginScreen onLogin={onLogin} />);

    fireEvent.click(await screen.findByRole('button', { name: /DJ/i }));

    await waitFor(() => expect(apiMock.auth.login).toHaveBeenCalledWith('2', undefined, false));
    expect(await screen.findByText(/Enter PIN if this user has one/i)).toBeInTheDocument();
    expect(onLogin).not.toHaveBeenCalled();
  });
});

