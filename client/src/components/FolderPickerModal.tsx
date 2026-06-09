import React, { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';

type Entry = { name: string; path: string };
type BrowseResult = { path: string; parent: string | null; entries: Entry[] };

type Props = {
  initialPath?: string;
  onSelect: (path: string) => void;
  onClose: () => void;
};

const S = {
  overlay: {
    position: 'fixed' as const,
    inset: 0,
    backgroundColor: 'rgba(0,0,0,0.6)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 1000,
  },
  modal: {
    backgroundColor: 'var(--surface)',
    border: '1px solid var(--border)',
    borderRadius: 10,
    width: 520,
    maxWidth: '95vw',
    maxHeight: '80vh',
    display: 'flex',
    flexDirection: 'column' as const,
    overflow: 'hidden',
  },
  header: {
    padding: '14px 16px 10px',
    borderBottom: '1px solid var(--border)',
  },
  title: { fontWeight: 700, fontSize: 15, color: 'var(--text)', marginBottom: 6 },
  breadcrumb: {
    fontSize: 11,
    color: 'var(--text-muted)',
    fontFamily: 'monospace',
    wordBreak: 'break-all' as const,
    lineHeight: 1.4,
  },
  toolbar: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    padding: '6px 16px',
    borderBottom: '1px solid var(--border)',
    flexWrap: 'wrap' as const,
  },
  upBtn: {
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    backgroundColor: 'var(--bg)',
    border: '1px solid var(--border)',
    borderRadius: 5,
    padding: '4px 10px',
    fontSize: 12,
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
  newFolderRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    padding: '6px 16px 8px',
    borderBottom: '1px solid var(--border)',
    backgroundColor: 'color-mix(in srgb, var(--surface) 60%, var(--bg))',
  },
  newFolderInput: {
    flex: 1,
    background: 'var(--bg)',
    border: '1px solid var(--accent)',
    color: 'var(--text)',
    borderRadius: 5,
    padding: '4px 8px',
    fontSize: 12,
    fontFamily: 'inherit',
    outline: 'none',
  },
  list: {
    flex: 1,
    overflowY: 'auto' as const,
    padding: '6px 8px',
  },
  entry: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '7px 10px',
    borderRadius: 5,
    cursor: 'pointer',
    fontSize: 13,
    color: 'var(--text)',
    userSelect: 'none' as const,
  },
  emptyMsg: {
    padding: '20px 16px',
    color: 'var(--text-muted)',
    fontSize: 12,
    textAlign: 'center' as const,
  },
  footer: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    gap: 8,
    padding: '10px 16px',
    borderTop: '1px solid var(--border)',
  },
  selectedPath: {
    fontSize: 11,
    color: 'var(--text-muted)',
    fontFamily: 'monospace',
    flex: 1,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap' as const,
  },
  btnRow: { display: 'flex', gap: 8 },
  btnPrimary: {
    backgroundColor: 'var(--accent)',
    color: '#fff',
    border: 'none',
    borderRadius: 6,
    padding: '7px 16px',
    cursor: 'pointer',
    fontSize: 13,
    fontWeight: 600,
    fontFamily: 'inherit',
  },
  btnSecondary: {
    backgroundColor: 'var(--bg)',
    color: 'var(--text-muted)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    padding: '7px 14px',
    cursor: 'pointer',
    fontSize: 13,
    fontFamily: 'inherit',
  },
  errorMsg: { padding: '8px 16px', color: '#ef4444', fontSize: 12 },
};

const FolderIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ flexShrink: 0, opacity: 0.6 }}>
    <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" />
  </svg>
);

export default function FolderPickerModal({ initialPath, onSelect, onClose }: Props) {
  const [result, setResult] = useState<BrowseResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [showNewFolder, setShowNewFolder] = useState(false);
  const [newFolderName, setNewFolderName] = useState('');
  const [newFolderError, setNewFolderError] = useState('');
  const [newFolderBusy, setNewFolderBusy] = useState(false);
  const newFolderInputRef = useRef<HTMLInputElement>(null);

  const navigate = useCallback(async (path?: string, fallbackToRoot = false) => {
    setLoading(true);
    setError('');
    setShowNewFolder(false);
    setNewFolderName('');
    setNewFolderError('');
    try {
      const data = await api.fsBrowse(path);
      setResult(data);
    } catch (e: any) {
      if (fallbackToRoot && path !== undefined) {
        // initial path is invalid — silently fall back to filesystem root
        try {
          const data = await api.fsBrowse(undefined);
          setResult(data);
        } catch (e2: any) {
          setError(e2.message || 'Failed to browse folder');
        }
      } else {
        setError(e.message || 'Failed to browse folder');
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    navigate(initialPath, true);
  }, [navigate, initialPath]);

  // Focus the new-folder input when the row appears.
  useEffect(() => {
    if (showNewFolder) newFolderInputRef.current?.focus();
  }, [showNewFolder]);

  const openNewFolder = () => {
    setNewFolderName('');
    setNewFolderError('');
    setShowNewFolder(true);
  };

  const cancelNewFolder = () => {
    setShowNewFolder(false);
    setNewFolderName('');
    setNewFolderError('');
  };

  const createFolder = async () => {
    const name = newFolderName.trim();
    if (!name) { setNewFolderError('Folder name cannot be empty'); return; }
    const parent = result?.path;
    if (!parent) return;
    setNewFolderBusy(true);
    setNewFolderError('');
    try {
      const created = await api.fsMkdir(parent, name);
      setShowNewFolder(false);
      setNewFolderName('');
      await navigate(created.path);
    } catch (e: any) {
      setNewFolderError(e.message || 'Failed to create folder');
    } finally {
      setNewFolderBusy(false);
    }
  };

  const currentPath = result?.path ?? '';

  return (
    <div style={S.overlay} onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div style={S.modal}>
        <div style={S.header}>
          <div style={S.title}>Choose a folder</div>
          {currentPath && <div style={S.breadcrumb}>{currentPath}</div>}
        </div>

        <div style={S.toolbar}>
          <button
            style={{ ...S.upBtn, opacity: result?.parent ? 1 : 0.4, cursor: result?.parent ? 'pointer' : 'default' }}
            disabled={!result?.parent}
            type="button"
            onClick={() => result?.parent && navigate(result.parent)}
          >
            ↑ Up
          </button>
          <button
            style={S.upBtn}
            type="button"
            onClick={() => navigate(undefined)}
          >
            / Root
          </button>
          <button
            style={{ ...S.upBtn, opacity: currentPath ? 1 : 0.4, cursor: currentPath ? 'pointer' : 'default' }}
            disabled={!currentPath || showNewFolder}
            type="button"
            onClick={openNewFolder}
          >
            + New Folder
          </button>
          {loading && <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>Loading…</span>}
        </div>

        {showNewFolder && (
          <div style={S.newFolderRow}>
            <FolderIcon />
            <input
              ref={newFolderInputRef}
              style={S.newFolderInput}
              type="text"
              placeholder="New folder name"
              value={newFolderName}
              onChange={e => { setNewFolderName(e.target.value); setNewFolderError(''); }}
              onKeyDown={e => {
                if (e.key === 'Enter') createFolder();
                if (e.key === 'Escape') cancelNewFolder();
              }}
              disabled={newFolderBusy}
            />
            <button
              style={{ ...S.upBtn, color: 'var(--accent)', borderColor: 'var(--accent)', opacity: newFolderBusy ? 0.5 : 1 }}
              type="button"
              disabled={newFolderBusy}
              onClick={createFolder}
            >
              {newFolderBusy ? 'Creating…' : 'Create'}
            </button>
            <button
              style={{ ...S.upBtn, opacity: newFolderBusy ? 0.5 : 1 }}
              type="button"
              disabled={newFolderBusy}
              onClick={cancelNewFolder}
            >
              ✕
            </button>
          </div>
        )}
        {newFolderError && <div style={S.errorMsg}>{newFolderError}</div>}

        {error && <div style={S.errorMsg}>{error}</div>}
        <div style={S.list}>
          {result && result.entries.length === 0 && !showNewFolder && (
            <div style={S.emptyMsg}>No subfolders</div>
          )}
          {result?.entries.map((entry) => (
            <div
              key={entry.path}
              style={S.entry}
              onClick={() => navigate(entry.path)}
              onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.backgroundColor = 'var(--bg)'; }}
              onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.backgroundColor = ''; }}
            >
              <FolderIcon />
              {entry.name}
            </div>
          ))}
        </div>
        <div style={S.footer}>
          <div style={S.selectedPath}>{currentPath || 'No folder selected'}</div>
          <div style={S.btnRow}>
            <button style={S.btnSecondary} type="button" onClick={onClose}>Cancel</button>
            <button
              style={{ ...S.btnPrimary, opacity: currentPath ? 1 : 0.5 }}
              type="button"
              disabled={!currentPath}
              onClick={() => currentPath && onSelect(currentPath)}
            >
              Select
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
