/**
 * Defines the User Management React component and related UI helpers.
 */

import React, { useState, useEffect, useRef } from 'react';
import { api } from '../api';
import { hybridControlStyles } from '../hybridPreview';
import type { AdminUser, AuthUser } from '../types';
import type { EntityId } from '../entityId';
import ConfirmModal from './ConfirmModal';

interface Props {
  currentUser: AuthUser;
}

const inputStyle: React.CSSProperties = { ...hybridControlStyles.field, minHeight: 36, padding: '6px 10px', fontSize: 15, width: '100%' };

const Icon = {
  Lock: () => (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <rect x="5" y="11" width="14" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 018 0v4" />
    </svg>
  ),
  Key: () => (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="8" cy="15" r="4" />
      <path d="M10.5 12.5L20 3M20 3h-4M20 3v4" />
    </svg>
  ),
  Trash: () => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14H6L5 6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
      <path d="M9 6V4h6v2" />
    </svg>
  ),
};

function PinModal({ userId, username, onClose }: { userId: EntityId; username: string; onClose: () => void }) {
  const [pin, setPin] = useState(['', '', '', '']);
  const [confirm, setConfirm] = useState(['', '', '', '']);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const refs = [useRef<HTMLInputElement>(null), useRef<HTMLInputElement>(null), useRef<HTMLInputElement>(null), useRef<HTMLInputElement>(null)];
  const crefs = [useRef<HTMLInputElement>(null), useRef<HTMLInputElement>(null), useRef<HTMLInputElement>(null), useRef<HTMLInputElement>(null)];

  const handleChange = (arr: string[], setArr: (v: string[]) => void, refs2: typeof refs, i: number, val: string) => {
    const digit = val.replace(/\D/g, '').slice(-1);
    const next = [...arr]; next[i] = digit; setArr(next);
    setError('');
    if (digit && i < 3) refs2[i + 1].current?.focus();
  };

  const handleKeyDown = (arr: string[], refs2: typeof refs, i: number, e: React.KeyboardEvent) => {
    if (e.key === 'Backspace' && !arr[i] && i > 0) refs2[i - 1].current?.focus();
  };

  const handleSave = async () => {
    const p = pin.join('');
    const c = confirm.join('');
    if (p.length !== 4) { setError('Enter a 4-digit PIN'); return; }
    if (p !== c) { setError('PINs do not match'); return; }
    setSaving(true);
    try {
      await api.admin.users.setPin(userId, p);
      onClose();
    } catch (e: any) {
      setError(e.message || 'Failed to set PIN');
    } finally {
      setSaving(false);
    }
  };

  const handleClear = async () => {
    setSaving(true);
    try {
      await api.admin.users.setPin(userId, null);
      onClose();
    } catch (e: any) {
      setError(e.message || 'Failed to clear PIN');
    } finally {
      setSaving(false);
    }
  };

  const PinRow = ({ arr, setArr, label, refs2 }: { arr: string[]; setArr: (v: string[]) => void; label: string; refs2: typeof refs }) => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>{label}</div>
      <div style={{ display: 'flex', gap: 8 }}>
        {arr.map((digit, i) => (
          <input
            key={i} ref={refs2[i]} type="password" inputMode="numeric" maxLength={1} value={digit}
            onChange={e => handleChange(arr, setArr, refs2, i, e.target.value)}
            onKeyDown={e => handleKeyDown(arr, refs2, i, e)}
            style={{ width: 44, height: 52, textAlign: 'center', fontSize: 24, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, color: 'var(--text)', outline: 'none', caretColor: 'transparent' }}
            onFocus={e => { e.currentTarget.style.borderColor = 'var(--accent)'; }}
            onBlur={e => { e.currentTarget.style.borderColor = 'var(--border)'; }}
          />
        ))}
      </div>
    </div>
  );

  return (
    <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.7)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 1000 }}>
      <div style={{ background: 'var(--bg)', border: '1px solid var(--border)', borderRadius: 12, padding: 28, minWidth: 300, display: 'flex', flexDirection: 'column', gap: 20 }}>
        <div style={{ fontSize: 17, fontWeight: 700 }}>Set PIN for {username}</div>
        <PinRow arr={pin} setArr={setPin} label="New PIN" refs2={refs} />
        <PinRow arr={confirm} setArr={setConfirm} label="Confirm PIN" refs2={crefs} />
        {error && <div style={{ color: 'var(--danger)', fontSize: 14 }}>{error}</div>}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button type="button" style={hybridControlStyles.dangerButton} onClick={handleClear} disabled={saving}>Clear PIN</button>
          <button type="button" style={hybridControlStyles.secondaryButton} onClick={onClose} disabled={saving}>Cancel</button>
          <button type="button" style={{ ...hybridControlStyles.primaryButton, ...(saving ? hybridControlStyles.disabled : {}) }} onClick={handleSave} disabled={saving}>Save</button>
        </div>
      </div>
    </div>
  );
}

const checkboxStyle: React.CSSProperties = { accentColor: 'var(--accent)', width: 14, height: 14, cursor: 'pointer' };

/** User Management is part of this module's public API. */
export default function UserManagement({ currentUser }: Props) {
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [loading, setLoading] = useState(true);
  const [newUsername, setNewUsername] = useState('');
  const [newRole, setNewRole] = useState<'user' | 'admin'>('user');
  const [newPin, setNewPin] = useState('');
  const [newCanManageLibraries, setNewCanManageLibraries] = useState(false);
  const [newCanEditMetadata, setNewCanEditMetadata] = useState(false);
  const [addError, setAddError] = useState('');
  const [pinModalUser, setPinModalUser] = useState<AdminUser | null>(null);
  // Doubles as the confirm-before-delete prompt and, with cancelLabel: null,
  // an acknowledged error dialog in place of window.alert.
  const [dialog, setDialog] = useState<{
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string | null;
    tone?: 'default' | 'danger';
    onConfirm: () => void;
  } | null>(null);
  const showError = (message: string) => setDialog({
    title: 'Something went wrong',
    message,
    confirmLabel: 'OK',
    cancelLabel: null,
    onConfirm: () => {},
  });
  const mountedRef = useRef(true);

  const load = () => {
    setLoading(true);
    api.admin.users.list()
      .then((nextUsers) => { if (mountedRef.current) setUsers(nextUsers); })
      .catch(() => {})
      .finally(() => { if (mountedRef.current) setLoading(false); });
  };

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => { mountedRef.current = false; };
  }, []);

  const handleAdd = async () => {
    if (!newUsername.trim()) { setAddError('Username required'); return; }
    if (newPin && !/^\d{4}$/.test(newPin)) { setAddError('PIN must be exactly 4 digits'); return; }
    setAddError('');
    try {
      await api.admin.users.create({
        username: newUsername.trim(), role: newRole, pin: newPin || undefined,
        canManageLibraries: newRole === 'user' ? newCanManageLibraries : undefined,
        canEditMetadata: newRole === 'user' ? newCanEditMetadata : undefined,
      });
      setNewUsername(''); setNewPin(''); setNewRole('user'); setNewCanManageLibraries(false); setNewCanEditMetadata(false);
      load();
    } catch (e: any) {
      setAddError(e.message || 'Failed to create user');
    }
  };

  const handleDelete = (user: AdminUser) => setDialog({
    title: `Delete user "${user.username}"?`,
    message: 'This cannot be undone.',
    confirmLabel: 'Delete',
    tone: 'danger',
    onConfirm: async () => {
      try {
        await api.admin.users.remove(user.id);
        load();
      } catch (e: any) {
        showError(e.message);
      }
    },
  });

  const handleTogglePermission = async (user: AdminUser, perm: 'canManageLibraries' | 'canEditMetadata') => {
    const next = { canManageLibraries: user.canManageLibraries, canEditMetadata: user.canEditMetadata, [perm]: !user[perm] };
    try {
      await api.admin.users.setPermissions(user.id, next);
      load();
    } catch (e: any) {
      showError(e.message);
    }
  };

  const sectionHead: React.CSSProperties = { fontSize: 18, fontWeight: 700, color: 'var(--text)', marginBottom: 12 };
  const permPill = (active: boolean, label: string) => (
    <span style={{
      fontSize: 12, padding: '2px 8px', borderRadius: 999, fontWeight: 600,
      border: '1px solid var(--border)',
      background: active ? 'var(--accent-soft)' : 'var(--surface-subtle)',
      color: active ? 'var(--accent)' : 'var(--text-muted)',
    }}>
      {label}
    </span>
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 28 }}>
      {/* Existing users */}
      <div>
        <div style={sectionHead}>Users</div>
        {loading ? (
          <div style={{ color: 'var(--text-muted)', fontSize: 15 }}>Loading...</div>
        ) : (
          <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
            {users.map((user, index) => (
              <div
                key={user.id}
                style={{
                  display: 'flex', alignItems: 'center', gap: 10, padding: '10px 14px',
                  borderTop: index > 0 ? '1px solid var(--border)' : undefined,
                }}
              >
                <div style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ fontSize: 15, fontWeight: 600, color: 'var(--text)' }}>{user.username}</span>
                  {user.id === currentUser.id && <span style={{ fontSize: 12, color: 'var(--accent)', marginLeft: 6 }}>(you)</span>}
                </div>
                <span style={{
                  fontSize: 12, padding: '2px 8px', borderRadius: 999, fontWeight: 600,
                  border: '1px solid var(--border)',
                  background: user.role === 'admin' ? 'var(--accent-soft)' : 'var(--surface-subtle)',
                  color: user.role === 'admin' ? 'var(--accent)' : 'var(--text-muted)',
                }}>
                  {user.role}
                </span>
                {user.role === 'user' && (
                  <>
                    <button
                      type="button"
                      style={{ background: 'none', border: 'none', padding: 0, cursor: 'pointer' }}
                      title="Toggle libraries management permission"
                      onClick={() => handleTogglePermission(user, 'canManageLibraries')}
                    >
                      {permPill(user.canManageLibraries, 'Libraries')}
                    </button>
                    <button
                      type="button"
                      style={{ background: 'none', border: 'none', padding: 0, cursor: 'pointer' }}
                      title="Toggle metadata edit permission"
                      onClick={() => handleTogglePermission(user, 'canEditMetadata')}
                    >
                      {permPill(user.canEditMetadata, 'Metadata')}
                    </button>
                  </>
                )}
                <span
                  title={user.hasPin ? 'Has a PIN' : 'No PIN set'}
                  style={{ display: 'inline-flex', alignItems: 'center', color: user.hasPin ? 'var(--accent)' : 'var(--text-muted)' }}
                >
                  <Icon.Lock />
                </span>
                <button
                  type="button"
                  style={hybridControlStyles.iconButton}
                  title={user.hasPin ? 'Change PIN' : 'Set PIN'}
                  aria-label={user.hasPin ? `Change PIN for ${user.username}` : `Set PIN for ${user.username}`}
                  onClick={() => setPinModalUser(user)}
                >
                  <Icon.Key />
                </button>
                <button
                  type="button"
                  style={{
                    ...hybridControlStyles.iconButton,
                    color: 'var(--danger)',
                    ...(user.id === currentUser.id ? hybridControlStyles.disabled : {}),
                  }}
                  disabled={user.id === currentUser.id}
                  title="Delete user"
                  aria-label={`Delete user ${user.username}`}
                  onClick={() => handleDelete(user)}
                >
                  <Icon.Trash />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Add user */}
      <div>
        <div style={sectionHead}>Add User</div>
        <div style={{ padding: '16px 20px', borderRadius: 8, backgroundColor: 'var(--surface)', border: '1px solid var(--border)' }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <input style={inputStyle} placeholder="Username" value={newUsername} onChange={e => { setNewUsername(e.target.value); setAddError(''); }} />
            <div style={{ display: 'flex', gap: 8 }}>
              <select
                style={{ ...hybridControlStyles.select, flex: 1 }}
                value={newRole}
                onChange={e => { setNewRole(e.target.value as 'user' | 'admin'); setNewCanManageLibraries(false); setNewCanEditMetadata(false); }}
              >
                <option value="user">User</option>
                <option value="admin">Admin</option>
              </select>
              <input
                style={{ ...inputStyle, flex: 1 }}
                placeholder="PIN (optional, 4 digits)"
                type="password"
                inputMode="numeric"
                maxLength={4}
                value={newPin}
                onChange={e => { setNewPin(e.target.value.replace(/\D/g, '').slice(0, 4)); setAddError(''); }}
              />
            </div>
            {newRole === 'user' && (
              <div style={{ display: 'flex', gap: 16 }}>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 14, color: 'var(--text-muted)', cursor: 'pointer' }}>
                  <input type="checkbox" style={checkboxStyle} checked={newCanManageLibraries} onChange={e => setNewCanManageLibraries(e.target.checked)} />
                  Allow libraries management
                </label>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 14, color: 'var(--text-muted)', cursor: 'pointer' }}>
                  <input type="checkbox" style={checkboxStyle} checked={newCanEditMetadata} onChange={e => setNewCanEditMetadata(e.target.checked)} />
                  Allow metadata editing
                </label>
              </div>
            )}
            <div style={{ fontSize: 13, color: 'var(--text-muted)' }}>
              PINs travel to the server in plain HTTP requests. Use remote access only on trusted networks.
            </div>
            {addError && <div style={{ color: 'var(--danger)', fontSize: 14 }}>{addError}</div>}
            <button type="button" style={{ ...hybridControlStyles.primaryButton, alignSelf: 'flex-start' }} onClick={handleAdd}>
              Add User
            </button>
          </div>
        </div>
      </div>

      {pinModalUser && (
        <PinModal
          userId={pinModalUser.id}
          username={pinModalUser.username}
          onClose={() => { setPinModalUser(null); load(); }}
        />
      )}

      {dialog && (
        <ConfirmModal
          title={dialog.title}
          message={dialog.message}
          confirmLabel={dialog.confirmLabel}
          cancelLabel={dialog.cancelLabel}
          tone={dialog.tone}
          onConfirm={() => { const { onConfirm } = dialog; setDialog(null); onConfirm(); }}
          onCancel={() => setDialog(null)}
        />
      )}
    </div>
  );
}
