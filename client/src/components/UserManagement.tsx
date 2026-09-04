/**
 * Defines the User Management React component and related UI helpers.
 */

import React, { useState, useEffect, useRef } from 'react';
import { api } from '../api';
import type { AdminUser, AuthUser } from '../types';
import type { EntityId } from '../entityId';
import ConfirmModal from './ConfirmModal';

interface Props {
  currentUser: AuthUser;
}

const inputStyle: React.CSSProperties = {
  background: 'var(--surface)', border: '1px solid var(--border)',
  color: 'var(--text)', borderRadius: 6, padding: '6px 10px',
  fontSize: 13, outline: 'none', width: '100%',
};

const btnStyle = (variant: 'primary' | 'danger' | 'ghost' = 'primary'): React.CSSProperties => ({
  padding: '6px 14px', borderRadius: 6, fontSize: 12, fontWeight: 600,
  cursor: 'pointer', border: 'none',
  background: variant === 'primary' ? 'var(--accent)' : variant === 'danger' ? '#ef4444' : 'var(--surface)',
  color: variant === 'ghost' ? 'var(--text-muted)' : '#fff',
});

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
      <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>{label}</div>
      <div style={{ display: 'flex', gap: 8 }}>
        {arr.map((digit, i) => (
          <input
            key={i} ref={refs2[i]} type="password" inputMode="numeric" maxLength={1} value={digit}
            onChange={e => handleChange(arr, setArr, refs2, i, e.target.value)}
            onKeyDown={e => handleKeyDown(arr, refs2, i, e)}
            style={{ width: 44, height: 52, textAlign: 'center', fontSize: 22, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8, color: 'var(--text)', outline: 'none', caretColor: 'transparent' }}
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
        <div style={{ fontSize: 15, fontWeight: 700 }}>Set PIN for {username}</div>
        <PinRow arr={pin} setArr={setPin} label="New PIN" refs2={refs} />
        <PinRow arr={confirm} setArr={setConfirm} label="Confirm PIN" refs2={crefs} />
        {error && <div style={{ color: '#ef4444', fontSize: 12 }}>{error}</div>}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button style={btnStyle('ghost')} onClick={handleClear} disabled={saving}>Clear PIN</button>
          <button style={btnStyle('ghost')} onClick={onClose} disabled={saving}>Cancel</button>
          <button style={btnStyle('primary')} onClick={handleSave} disabled={saving}>Save</button>
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

  const sectionHead: React.CSSProperties = { fontSize: 12, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 1, marginBottom: 12 };
  const permTag = (active: boolean, label: string) => (
    <span style={{ fontSize: 10, padding: '2px 7px', borderRadius: 4, background: active ? 'rgba(99,102,241,0.2)' : 'rgba(255,255,255,0.04)', color: active ? 'var(--accent)' : 'var(--text-muted)', fontWeight: 600 }}>
      {label}
    </span>
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 28 }}>
      {/* Existing users */}
      <div>
        <div style={sectionHead}>Users</div>
        {loading ? (
          <div style={{ color: 'var(--text-muted)', fontSize: 13 }}>Loading...</div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {users.map(user => (
              <div key={user.id} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '10px 14px', background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 8 }}>
                <div style={{ flex: 1 }}>
                  <span style={{ fontSize: 13, fontWeight: 600 }}>{user.username}</span>
                  {user.id === currentUser.id && <span style={{ fontSize: 10, color: 'var(--accent)', marginLeft: 6 }}>(you)</span>}
                </div>
                <span style={{ fontSize: 10, padding: '2px 8px', borderRadius: 4, background: user.role === 'admin' ? 'rgba(99,102,241,0.2)' : 'rgba(255,255,255,0.06)', color: user.role === 'admin' ? 'var(--accent)' : 'var(--text-muted)', fontWeight: 600 }}>
                  {user.role}
                </span>
                {user.role === 'user' && (
                  <>
                    <button style={{ ...btnStyle('ghost'), padding: '3px 8px', fontSize: 10 }} title="Toggle libraries management permission" onClick={() => handleTogglePermission(user, 'canManageLibraries')}>
                      {permTag(user.canManageLibraries, 'Libraries')}
                    </button>
                    <button style={{ ...btnStyle('ghost'), padding: '3px 8px', fontSize: 10 }} title="Toggle metadata edit permission" onClick={() => handleTogglePermission(user, 'canEditMetadata')}>
                      {permTag(user.canEditMetadata, 'Metadata')}
                    </button>
                  </>
                )}
                <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>{user.hasPin ? '🔒 PIN' : 'No PIN'}</span>
                <button style={btnStyle('ghost')} onClick={() => setPinModalUser(user)}>
                  {user.hasPin ? 'Change PIN' : 'Set PIN'}
                </button>
                <button
                  style={{ ...btnStyle('danger'), opacity: user.id === currentUser.id ? 0.4 : 1 }}
                  disabled={user.id === currentUser.id}
                  onClick={() => handleDelete(user)}
                >
                  Delete
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Add user */}
      <div>
        <div style={sectionHead}>Add User</div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <input style={inputStyle} placeholder="Username" value={newUsername} onChange={e => { setNewUsername(e.target.value); setAddError(''); }} />
          <div style={{ display: 'flex', gap: 8 }}>
            <select
              style={{ ...inputStyle, flex: 1 }}
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
              <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--text-muted)', cursor: 'pointer' }}>
                <input type="checkbox" style={checkboxStyle} checked={newCanManageLibraries} onChange={e => setNewCanManageLibraries(e.target.checked)} />
                Allow libraries management
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--text-muted)', cursor: 'pointer' }}>
                <input type="checkbox" style={checkboxStyle} checked={newCanEditMetadata} onChange={e => setNewCanEditMetadata(e.target.checked)} />
                Allow metadata editing
              </label>
            </div>
          )}
          <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
            PINs travel to the server in plain HTTP requests. Use remote access only on trusted networks.
          </div>
          {addError && <div style={{ color: '#ef4444', fontSize: 12 }}>{addError}</div>}
          <button style={{ ...btnStyle('primary'), alignSelf: 'flex-start' }} onClick={handleAdd}>
            Add User
          </button>
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
