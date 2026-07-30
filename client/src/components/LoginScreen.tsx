/**
 * Defines the Login Screen React component and related UI helpers.
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles, hybridEntryStyles } from '../hybridPreview';
import type { AuthUser, LoginUser } from '../types';

interface Props {
  onLogin: (user: AuthUser) => void;
}

function UserAvatar({ username, size = 64 }: { username: string; size?: number }) {
  return (
    <div
      aria-hidden="true"
      style={{
        ...hybridEntryStyles.avatar,
        width: size,
        height: size,
        borderRadius: size * 0.34,
        fontSize: size * 0.32,
      }}
    >
      {username.slice(0, 2).toUpperCase()}
    </div>
  );
}

/** Login Screen is part of this module's public API. */
export default function LoginScreen({ onLogin }: Props) {
  const [users, setUsers] = useState<LoginUser[]>([]);
  const [selected, setSelected] = useState<LoginUser | null>(null);
  const [pin, setPin] = useState<string[]>(['', '', '', '']);
  const [stayLoggedIn, setStayLoggedIn] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [profilesLoading, setProfilesLoading] = useState(true);
  const [activeUserId, setActiveUserId] = useState<string | null>(null);
  const pinRefs = useMemo(
    () => [
      React.createRef<HTMLInputElement>(),
      React.createRef<HTMLInputElement>(),
      React.createRef<HTMLInputElement>(),
      React.createRef<HTMLInputElement>(),
    ],
    [],
  );

  useEffect(() => {
    api.auth.getLoginUsers()
      .then(setUsers)
      .catch(() => {})
      .finally(() => setProfilesLoading(false));
  }, []);

  const handleSelectUser = useCallback(async (user: LoginUser) => {
    setError('');
    setLoading(true);
    try {
      const { user: authed } = await api.auth.login(user.id, undefined, stayLoggedIn);
      onLogin(authed);
      return;
    } catch {
      setSelected(user);
      setPin(['', '', '', '']);
      setError('');
      setTimeout(() => pinRefs[0].current?.focus(), 50);
    } finally {
      setLoading(false);
    }
  }, [onLogin, pinRefs, stayLoggedIn]);

  const submitPin = async (pinValue: string) => {
    if (!selected) return;
    setLoading(true);
    setError('');
    try {
      const normalizedPin = pinValue.trim();
      const { user: authed } = await api.auth.login(
        selected.id,
        normalizedPin || undefined,
        stayLoggedIn,
      );
      onLogin(authed);
    } catch (loginError: any) {
      setError(loginError?.message || 'Invalid credentials');
      setPin(['', '', '', '']);
      setTimeout(() => pinRefs[0].current?.focus(), 50);
    } finally {
      setLoading(false);
    }
  };

  const handlePinChange = (index: number, value: string) => {
    const digit = value.replace(/\D/g, '').slice(-1);
    const next = [...pin];
    next[index] = digit;
    setPin(next);
    setError('');
    if (digit && index < 3) pinRefs[index + 1].current?.focus();
    if (next.every(candidate => candidate !== '') && index === 3) {
      void submitPin(next.join(''));
    }
  };

  const handlePinKeyDown = (index: number, event: React.KeyboardEvent) => {
    if (event.key === 'Backspace' && !pin[index] && index > 0) {
      pinRefs[index - 1].current?.focus();
    }
  };

  const handleBack = () => {
    setSelected(null);
    setPin(['', '', '', '']);
    setError('');
  };

  return (
    <main style={hybridEntryStyles.viewport}>
      <section
        aria-busy={loading}
        aria-labelledby="login-title"
        style={hybridEntryStyles.card}
      >
        <div style={hybridEntryStyles.brand}>
          <img src="/boogiebox.png" alt="" style={hybridEntryStyles.logo} />
          <div>
            <div style={hybridEntryStyles.brandName}>BoogieBox</div>
            {selected ? (
              <div style={hybridEntryStyles.brandMeta}>
                Logging in as {selected.username}
              </div>
            ) : null}
          </div>
        </div>

        <header>
          <div style={hybridEntryStyles.eyebrow}>
            {selected ? 'Secure sign in' : 'Welcome back'}
          </div>
          <h1 id="login-title" style={hybridEntryStyles.title}>
            {selected ? 'Enter your PIN' : 'Select a user'}
          </h1>
          <p style={hybridEntryStyles.description}>
            {selected
              ? 'Enter PIN if this user has one. Passwordless profiles can continue without a code.'
              : 'Choose your profile to continue to your BoogieBox library.'}
          </p>
        </header>

        <div style={hybridEntryStyles.content}>
          {!selected ? (
            <>
              {profilesLoading ? (
                <div role="status" style={hybridEntryStyles.empty}>Loading profiles…</div>
              ) : users.length > 0 ? (
                <div aria-label="User profiles" style={hybridEntryStyles.userGrid}>
                  {users.map(user => {
                    const isActive = activeUserId === user.id;
                    return (
                      <button
                        key={user.id}
                        type="button"
                        aria-label={`Sign in as ${user.username}`}
                        onClick={() => void handleSelectUser(user)}
                        onMouseEnter={() => setActiveUserId(user.id)}
                        onMouseLeave={() => setActiveUserId(null)}
                        onFocus={() => setActiveUserId(user.id)}
                        onBlur={() => setActiveUserId(null)}
                        disabled={loading}
                        style={{
                          ...hybridEntryStyles.userButton,
                          ...(isActive ? hybridEntryStyles.userButtonActive : {}),
                          ...(loading ? hybridControlStyles.disabled : {}),
                        }}
                      >
                        <UserAvatar username={user.username} />
                        <span style={hybridEntryStyles.userName}>{user.username}</span>
                      </button>
                    );
                  })}
                </div>
              ) : (
                <div role="status" style={hybridEntryStyles.empty}>
                  No user profiles are available yet. If the server is still starting, try again
                  in a moment.
                </div>
              )}

              <div style={{ ...hybridEntryStyles.optionRow, justifyContent: 'center' }}>
                <label style={hybridEntryStyles.checkboxLabel}>
                  <input
                    type="checkbox"
                    checked={stayLoggedIn}
                    onChange={event => setStayLoggedIn(event.target.checked)}
                    style={hybridEntryStyles.checkbox}
                  />
                  Stay logged in
                </label>
              </div>
            </>
          ) : (
            <form
              onSubmit={event => {
                event.preventDefault();
                void submitPin(pin.join(''));
              }}
            >
              <div style={hybridEntryStyles.profile}>
                <UserAvatar username={selected.username} size={58} />
                <div style={{ minWidth: 0 }}>
                  <div style={{ ...hybridEntryStyles.userName, textAlign: 'left' }}>
                    {selected.username}
                  </div>
                  <div style={hybridEntryStyles.profileMeta}>Four-digit access PIN</div>
                </div>
              </div>

              <div aria-label="PIN" style={hybridEntryStyles.pinRow}>
                {pin.map((digit, index) => (
                  <input
                    key={index}
                    ref={pinRefs[index]}
                    type="password"
                    inputMode="numeric"
                    autoComplete={index === 0 ? 'one-time-code' : 'off'}
                    aria-label={`PIN digit ${index + 1}`}
                    aria-invalid={Boolean(error)}
                    maxLength={1}
                    value={digit}
                    onChange={event => handlePinChange(index, event.target.value)}
                    onKeyDown={event => handlePinKeyDown(index, event)}
                    onFocus={event => {
                      event.currentTarget.style.borderColor = error
                        ? 'var(--danger)'
                        : 'var(--focus)';
                      event.currentTarget.style.boxShadow = '0 0 0 3px var(--focus-ring)';
                    }}
                    onBlur={event => {
                      event.currentTarget.style.borderColor = error
                        ? 'var(--danger)'
                        : 'var(--border)';
                      event.currentTarget.style.boxShadow = 'none';
                    }}
                    disabled={loading}
                    style={{
                      ...hybridEntryStyles.pinField,
                      ...(error ? { borderColor: 'var(--danger)' } : {}),
                    }}
                  />
                ))}
              </div>

              {error && (
                <div role="alert" style={hybridEntryStyles.error}>
                  {error}
                </div>
              )}

              <div style={hybridEntryStyles.optionRow}>
                <label style={hybridEntryStyles.checkboxLabel}>
                  <input
                    type="checkbox"
                    checked={stayLoggedIn}
                    onChange={event => setStayLoggedIn(event.target.checked)}
                    style={hybridEntryStyles.checkbox}
                  />
                  Stay logged in
                </label>
                <button
                  type="button"
                  onClick={handleBack}
                  disabled={loading}
                  style={{
                    ...hybridControlStyles.secondaryButton,
                    ...(loading ? hybridControlStyles.disabled : {}),
                  }}
                >
                  ← Back to users
                </button>
              </div>

              <button
                type="submit"
                disabled={loading}
                style={{
                  ...hybridControlStyles.primaryButton,
                  width: '100%',
                  marginTop: 14,
                  ...(loading ? hybridControlStyles.disabled : {}),
                }}
              >
                {loading ? 'Signing in…' : 'Continue'}
              </button>
            </form>
          )}
        </div>

      </section>
    </main>
  );
}
