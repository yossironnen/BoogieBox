/**
 * Defines the Login Screen React component and related UI helpers.
 */

import React, { useState, useEffect, useMemo, useCallback } from 'react';
import { api } from '../api';
import type { AuthUser, LoginUser } from '../types';

interface Props {
  onLogin: (user: AuthUser) => void;
}

function UserAvatar({ username, size = 64 }: { username: string; size?: number }) {
  const initials = username.slice(0, 2).toUpperCase();
  const hue = Array.from(username).reduce((acc, c) => acc + c.charCodeAt(0), 0) % 360;
  return (
    <div style={{
      width: size, height: size, borderRadius: '50%',
      background: `hsl(${hue}, 55%, 32%)`,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      fontSize: size * 0.36, fontWeight: 700, color: `hsl(${hue}, 80%, 90%)`,
      flexShrink: 0,
    }}>
      {initials}
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
  const pinRefs = useMemo(
    () => [React.createRef<HTMLInputElement>(), React.createRef<HTMLInputElement>(), React.createRef<HTMLInputElement>(), React.createRef<HTMLInputElement>()],
    [],
  );

  useEffect(() => {
    api.auth.getLoginUsers().then(setUsers).catch(() => {});
  }, []);

  const handleSelectUser = useCallback(async (user: LoginUser) => {
    setError('');
    setLoading(true);
    try {
      const { user: authed } = await api.auth.login(user.id, undefined, stayLoggedIn);
      onLogin(authed);
      return;
    } catch (e: any) {
      setSelected(user);
      setPin(['', '', '', '']);
      setError('');
      setTimeout(() => pinRefs[0].current?.focus(), 50);
    } finally {
      setLoading(false);
    }
  }, [onLogin, pinRefs, stayLoggedIn]);

  const handlePinChange = (i: number, val: string) => {
    const digit = val.replace(/\D/g, '').slice(-1);
    const next = [...pin];
    next[i] = digit;
    setPin(next);
    setError('');
    if (digit && i < 3) pinRefs[i + 1].current?.focus();
    if (next.every(d => d !== '') && i === 3) {
      submitPin(next.join(''));
    }
  };

  const handlePinKeyDown = (i: number, e: React.KeyboardEvent) => {
    if (e.key === 'Backspace' && !pin[i] && i > 0) {
      pinRefs[i - 1].current?.focus();
    }
  };

  const submitPin = async (pinValue: string) => {
    if (!selected) return;
    setLoading(true);
    setError('');
    try {
      const normalizedPin = pinValue.trim();
      const { user: authed } = await api.auth.login(selected.id, normalizedPin || undefined, stayLoggedIn);
      onLogin(authed);
    } catch (e: any) {
      setError(e?.message || 'Invalid credentials');
      setPin(['', '', '', '']);
      setTimeout(() => pinRefs[0].current?.focus(), 50);
    } finally {
      setLoading(false);
    }
  };

  const handleBack = () => {
    setSelected(null);
    setPin(['', '', '', '']);
    setError('');
  };

  const containerStyle: React.CSSProperties = {
    minHeight: '100vh', display: 'flex', flexDirection: 'column',
    alignItems: 'center', justifyContent: 'center',
    background: '#09090b', color: '#e4e4e7',
    fontFamily: 'IBM Plex Mono, monospace',
    padding: 24,
  };

  const cardStyle: React.CSSProperties = {
    background: '#111113', border: '1px solid #27272a', borderRadius: 16,
    padding: '40px 48px', maxWidth: 480, width: '100%',
    display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 32,
  };

  return (
    <div style={containerStyle}>
      <div style={cardStyle}>
        {/* Logo */}
        <div style={{ textAlign: 'center' }}>
          <div style={{ fontSize: 28, fontWeight: 800, letterSpacing: '-0.5px', color: '#6366f1' }}>
            BoogieBox
          </div>
          <div style={{ fontSize: 12, color: '#71717a', marginTop: 4 }}>
            {selected ? `Logging in as ${selected.username}` : 'Select a user'}
          </div>
        </div>

        {!selected ? (
          <>
            {/* User grid */}
            <div style={{
              display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(100px, 1fr))',
              gap: 16, width: '100%',
            }}>
              {users.map(user => (
                <button
                  key={user.id}
                  onClick={() => handleSelectUser(user)}
                  disabled={loading}
                  style={{
                    background: 'transparent', border: '1px solid #27272a', borderRadius: 12,
                    padding: '16px 8px', cursor: 'pointer', display: 'flex',
                    flexDirection: 'column', alignItems: 'center', gap: 10,
                    color: '#e4e4e7', transition: 'border-color 0.15s, background 0.15s',
                  }}
                  onMouseEnter={e => { (e.currentTarget as HTMLElement).style.borderColor = '#6366f1'; (e.currentTarget as HTMLElement).style.background = '#1a1a1f'; }}
                  onMouseLeave={e => { (e.currentTarget as HTMLElement).style.borderColor = '#27272a'; (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
                >
                  <UserAvatar username={user.username} />
                  <div style={{ fontSize: 12, fontWeight: 600, textAlign: 'center', wordBreak: 'break-word' }}>
                    {user.username}
                  </div>
                </button>
              ))}
            </div>

            {/* Stay logged in */}
            <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 13, color: '#71717a' }}>
              <input
                type="checkbox"
                checked={stayLoggedIn}
                onChange={e => setStayLoggedIn(e.target.checked)}
                style={{ width: 16, height: 16, accentColor: '#6366f1' }}
              />
              Stay logged in
            </label>
          </>
        ) : (
          <>
            {/* PIN entry */}
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 20 }}>
              <UserAvatar username={selected.username} size={80} />
              <div style={{ fontSize: 15, fontWeight: 600 }}>{selected.username}</div>
              <div style={{ fontSize: 12, color: '#71717a' }}>Enter PIN if this user has one</div>
              <div style={{ display: 'flex', gap: 12 }}>
                {pin.map((digit, i) => (
                  <input
                    key={i}
                    ref={pinRefs[i]}
                    type="password"
                    inputMode="numeric"
                    maxLength={1}
                    value={digit}
                    onChange={e => handlePinChange(i, e.target.value)}
                    onKeyDown={e => handlePinKeyDown(i, e)}
                    disabled={loading}
                    style={{
                      width: 52, height: 60, textAlign: 'center', fontSize: 24,
                      background: '#09090b', border: `1px solid ${error ? '#ef4444' : '#27272a'}`,
                      borderRadius: 10, color: '#e4e4e7', outline: 'none',
                      caretColor: 'transparent',
                    }}
                    onFocus={e => { e.currentTarget.style.borderColor = error ? '#ef4444' : '#6366f1'; }}
                    onBlur={e => { e.currentTarget.style.borderColor = error ? '#ef4444' : '#27272a'; }}
                  />
                ))}
              </div>
              <button
                onClick={() => submitPin(pin.join(''))}
                disabled={loading}
                style={{
                  minWidth: 140,
                  height: 40,
                  borderRadius: 999,
                  border: '1px solid #6366f1',
                  background: '#6366f1',
                  color: '#f4f4f5',
                  fontSize: 13,
                  fontWeight: 700,
                  cursor: loading ? 'wait' : 'pointer',
                  opacity: loading ? 0.75 : 1,
                }}
              >
                {loading ? 'Signing in...' : 'Continue'}
              </button>
              {error && <div style={{ color: '#ef4444', fontSize: 12 }}>{error}</div>}
            </div>

            {/* Stay logged in + back */}
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12, width: '100%' }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 13, color: '#71717a' }}>
                <input
                  type="checkbox"
                  checked={stayLoggedIn}
                  onChange={e => setStayLoggedIn(e.target.checked)}
                  style={{ width: 16, height: 16, accentColor: '#6366f1' }}
                />
                Stay logged in
              </label>
              <button
                onClick={handleBack}
                disabled={loading}
                style={{
                  background: 'transparent', border: 'none', color: '#71717a',
                  cursor: 'pointer', fontSize: 12, textDecoration: 'underline',
                }}
              >
                ← Back to users
              </button>
            </div>
          </>
        )}

        {error && !selected && <div style={{ color: '#ef4444', fontSize: 12 }}>{error}</div>}
      </div>
    </div>
  );
}
