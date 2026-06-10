/**
 * Defines the Setup View React component and related UI helpers.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { platform } from '../platform';
import FolderPickerModal from './FolderPickerModal';

const DEFAULT_DATABASE_FOLDER = 'C:\\Users\\Public\\BoogieBox';

/** Setup View is part of this module's public API. */
export default function SetupView({ onComplete }: { onComplete: () => void }) {
  const [folder, setFolder] = useState(DEFAULT_DATABASE_FOLDER);
  const [loading, setLoading] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerInitial, setPickerInitial] = useState<string | undefined>(undefined);
  const pickerResolveRef = useRef<((path: string | null) => void) | null>(null);
  const userEditedFolderRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    api.systemStatus()
      .then(status => {
        const suggested = status.suggestedDbFolder?.trim();
        if (!cancelled && suggested && !userEditedFolderRef.current) setFolder(suggested);
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!folder.trim()) return;
    setLoading(true);
    setError(null);
    try {
      await api.systemSetup(folder.trim());
      onComplete();
    } catch (err: any) {
      setError(err.message || 'Setup failed');
    } finally {
      setLoading(false);
    }
  };

  const handleBrowse = async () => {
    if (platform.isDesktop) {
      setBrowsing(true);
      setError(null);
      try {
        const selected = await platform.selectFolder(folder.trim() || DEFAULT_DATABASE_FOLDER);
        if (selected) setFolder(selected);
      } catch (err: any) {
        setError(err.message || 'Folder picker failed');
      } finally {
        setBrowsing(false);
      }
      return;
    }
    const selected = await new Promise<string | null>((resolve) => {
      pickerResolveRef.current = resolve;
      setPickerInitial(folder.trim() || undefined);
      setPickerOpen(true);
    });
    if (selected) setFolder(selected);
  };

  return (
    <>
    {pickerOpen && (
      <FolderPickerModal
        initialPath={pickerInitial}
        onSelect={(path) => {
          setPickerOpen(false);
          pickerResolveRef.current?.(path);
          pickerResolveRef.current = null;
        }}
        onClose={() => {
          setPickerOpen(false);
          pickerResolveRef.current?.(null);
          pickerResolveRef.current = null;
        }}
      />
    )}
    <div style={{
      position: 'fixed', inset: 0,
      background: 'var(--bg)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      fontFamily: 'var(--font), monospace',
      color: 'var(--text)',
    }}>
      <div style={{
        width: '100%', maxWidth: 480,
        background: 'var(--surface)',
        border: '1px solid var(--border)',
        borderRadius: 12,
        padding: '36px 40px',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 28 }}>
          <img src="/boogiebox.png" alt="BoogieBox" style={{ width: 36, height: 36, borderRadius: 8 }} />
          <div>
            <div style={{ fontSize: 18, fontWeight: 700 }}>Welcome to BoogieBox</div>
            <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2 }}>First-time setup</div>
          </div>
        </div>

        <p style={{ fontSize: 13, color: 'var(--text-muted)', marginBottom: 24, lineHeight: 1.6 }}>
          Choose a folder where BoogieBox will store its database.
          This can be a local path or a network path (UNC).
        </p>

        <form onSubmit={handleSubmit}>
          <label htmlFor="setup-database-folder" style={{ fontSize: 11, fontWeight: 700, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: 1 }}>
            Database folder
          </label>
          <div style={{ display: 'flex', gap: 8, marginTop: 8, marginBottom: 16 }}>
            <input
              id="setup-database-folder"
              type="text"
              value={folder}
              onChange={e => {
                userEditedFolderRef.current = true;
                setFolder(e.target.value);
              }}
              placeholder="e.g. C:\BoogieBox\data or \\server\share\boogieboxdb"
              autoFocus
              disabled={loading}
              style={{
                flex: 1, minWidth: 0,
                background: 'var(--bg)', border: '1px solid var(--border)',
                borderRadius: 6, padding: '10px 12px',
                color: 'var(--text)', fontSize: 13,
                fontFamily: 'var(--font), monospace',
                boxSizing: 'border-box',
              }}
            />
            <button
              type="button"
              onClick={handleBrowse}
              disabled={loading || browsing}
              style={{
                flex: '0 0 auto',
                minWidth: 88,
                background: 'var(--surface)',
                border: '1px solid var(--border)',
                borderRadius: 6,
                padding: '0 14px',
                color: 'var(--text)',
                fontSize: 13,
                fontWeight: 700,
                cursor: loading || browsing ? 'not-allowed' : 'pointer',
                fontFamily: 'var(--font), monospace',
              }}
            >
              {browsing ? 'Opening...' : 'Browse'}
            </button>
          </div>

          {error && (
            <div style={{
              marginBottom: 16, padding: '10px 12px',
              background: 'color-mix(in srgb, #ef4444 12%, var(--surface))',
              border: '1px solid color-mix(in srgb, #ef4444 40%, var(--border))',
              borderRadius: 6, fontSize: 12, color: '#f87171',
            }}>
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={loading || !folder.trim()}
            style={{
              width: '100%', padding: '10px 0',
              background: folder.trim() && !loading ? 'var(--accent)' : 'color-mix(in srgb, var(--accent) 40%, var(--surface))',
              border: 'none', borderRadius: 6,
              color: '#fff', fontSize: 13, fontWeight: 700,
              cursor: loading || !folder.trim() ? 'not-allowed' : 'pointer',
              fontFamily: 'var(--font), monospace',
            }}
          >
            {loading ? 'Setting up…' : 'Set up BoogieBox'}
          </button>
        </form>
      </div>
    </div>
    </>
  );
}
