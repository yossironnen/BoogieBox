import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../api';
import LoginScreen from './LoginScreen';

const users = [
  { id: 'user-1', username: 'alice' },
  { id: 'user-2', username: 'bob' },
] as any[];

describe('LoginScreen', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(api.auth, 'getLoginUsers').mockResolvedValue(users);
    vi.spyOn(api.auth, 'login');
  });

  it('loads users and completes passwordless login with persistence', async () => {
    const onLogin = vi.fn();
    vi.mocked(api.auth.login).mockResolvedValue({
      user: { id: 'user-1', username: 'alice', role: 'user' } as any,
    });
    render(<LoginScreen onLogin={onLogin} />);
    expect(await screen.findByRole('heading', { name: 'Select a user' })).toBeInTheDocument();
    expect(screen.queryByText('Private music, your way')).not.toBeInTheDocument();
    const alice = await screen.findByRole('button', { name: /alice/i });
    fireEvent.mouseEnter(alice);
    fireEvent.mouseLeave(alice);
    fireEvent.click(screen.getByLabelText('Stay logged in'));
    fireEvent.click(alice);

    await waitFor(() => expect(api.auth.login).toHaveBeenCalledWith('user-1', undefined, true));
    expect(onLogin).toHaveBeenCalledWith(expect.objectContaining({ username: 'alice' }));
  });

  it('falls back to PIN entry, validates failures, retries, and returns to users', async () => {
    vi.mocked(api.auth.login)
      .mockRejectedValueOnce(new Error('PIN required'))
      .mockRejectedValueOnce(new Error('Wrong PIN'))
      .mockResolvedValueOnce({
        user: { id: 'user-2', username: 'bob', role: 'user' } as any,
      });
    const onLogin = vi.fn();
    render(<LoginScreen onLogin={onLogin} />);
    fireEvent.click(await screen.findByRole('button', { name: /bob/i }));
    expect(await screen.findByText('Logging in as bob')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Enter your PIN' })).toBeInTheDocument();
    expect(screen.getByLabelText('PIN digit 1')).toHaveAttribute('inputmode', 'numeric');

    ['1', '2', '3', '4'].forEach((digit, index) => {
      const inputs = document.querySelectorAll<HTMLInputElement>('input[type="password"]');
      fireEvent.change(inputs[index], { target: { value: digit } });
    });
    expect(await screen.findByRole('alert')).toHaveTextContent('Wrong PIN');

    const cleared = document.querySelectorAll<HTMLInputElement>('input[type="password"]');
    fireEvent.keyDown(cleared[1], { key: 'Backspace' });
    fireEvent.focus(cleared[0]);
    fireEvent.blur(cleared[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));
    await waitFor(() => expect(onLogin).toHaveBeenCalled());

    fireEvent.click(screen.getByRole('button', { name: /Back to users/ }));
    expect(screen.getByText('Select a user')).toBeInTheDocument();
  });

  it('keeps the user chooser usable when user discovery fails', async () => {
    vi.mocked(api.auth.getLoginUsers).mockRejectedValue(new Error('offline'));
    render(<LoginScreen onLogin={vi.fn()} />);
    expect(screen.getByRole('status')).toHaveTextContent('Loading profiles');
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('No user profiles are available yet'));
    expect(screen.getByText('Select a user')).toBeInTheDocument();
  });
});
