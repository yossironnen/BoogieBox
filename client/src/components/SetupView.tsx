/**
 * Defines the Setup View React component and related UI helpers.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles, hybridEntryStyles } from '../hybridPreview';
import { platform } from '../platform';
import FolderPickerModal from './FolderPickerModal';
import SetupRestoreForm from './SetupRestoreForm';
import { BackupIcons } from './backupUi';

const DEFAULT_DATABASE_FOLDER = 'C:\\Users\\Public\\BoogieBox';

/** Setup View is part of this module's public API. */
export default function SetupView({ onComplete }: { onComplete: () => void }) {
  const [folder, setFolder] = useState(DEFAULT_DATABASE_FOLDER);
  const [loading, setLoading] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerInitial, setPickerInitial] = useState<string | undefined>(undefined);
  const [mode, setMode] = useState<'new' | 'restore'>('new');
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
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!folder.trim()) return;
    setLoading(true);
    setError(null);
    try {
      await api.systemSetup(folder.trim());
      onComplete();
    } catch (setupError: any) {
      setError(setupError.message || 'Setup failed');
    } finally {
      setLoading(false);
    }
  };

  /** The desktop app's native picker, otherwise the server folder browser. */
  const pickFolder = async (initial: string): Promise<string | null> => {
    if (platform.isDesktop) {
      setBrowsing(true);
      setError(null);
      try {
        return await platform.selectFolder(initial || DEFAULT_DATABASE_FOLDER);
      } catch (pickerError: any) {
        setError(pickerError.message || 'Folder picker failed');
        return null;
      } finally {
        setBrowsing(false);
      }
    }
    return new Promise<string | null>(resolve => {
      pickerResolveRef.current = resolve;
      setPickerInitial(initial || undefined);
      setPickerOpen(true);
    });
  };

  const handleBrowse = async () => {
    const selected = await pickFolder(folder.trim());
    if (selected) {
      userEditedFolderRef.current = true;
      setFolder(selected);
    }
  };

  const setupDisabled = loading || !folder.trim();

  return (
    <>
      {pickerOpen && (
        <FolderPickerModal
          initialPath={pickerInitial}
          onSelect={path => {
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

      <main
        style={{
          ...hybridEntryStyles.viewport,
          position: 'fixed',
          inset: 0,
        }}
      >
        <section
          aria-busy={loading}
          aria-labelledby="setup-title"
          style={hybridEntryStyles.card}
        >
          <div style={hybridEntryStyles.brand}>
            <img src="/boogiebox.png" alt="" style={hybridEntryStyles.logo} />
            <div>
              <div style={hybridEntryStyles.brandName}>BoogieBox</div>
              <div style={hybridEntryStyles.brandMeta}>First-time setup</div>
            </div>
          </div>

          <header>
            <div style={hybridEntryStyles.eyebrow}>Welcome</div>
            <h1 id="setup-title" style={hybridEntryStyles.title}>Set up your server</h1>
            {mode === 'new' && (
              <p style={hybridEntryStyles.description}>
                Choose where BoogieBox will keep its database. Your music stays in its existing
                folders.
              </p>
            )}
          </header>

          <div role="radiogroup" aria-label="How to start" style={S.choices}>
            {([
              ['new', 'New server', 'Start with an empty database.', <BackupIcons.Plus key="i" size={16} />],
              ['restore', 'Restore from a backup', 'Bring users, playlists, history and settings from another BoogieBox.', <BackupIcons.Restore key="i" size={16} />],
            ] as const).map(([value, title, text, icon]) => {
              const active = mode === value;
              return (
                <label key={value} style={{ ...S.choice, ...(active ? S.choiceActive : null) }}>
                  <input
                    type="radio"
                    name="setup-start"
                    checked={active}
                    onChange={() => setMode(value)}
                    disabled={loading}
                    style={{ marginTop: 3, accentColor: 'var(--accent)' }}
                  />
                  <span style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, fontSize: 15, fontWeight: 700, color: active ? 'var(--accent)' : 'var(--text)' }}>
                      {icon}{title}
                    </span>
                    <span style={{ fontSize: 13, color: 'var(--text-muted)', lineHeight: 1.5 }}>{text}</span>
                  </span>
                </label>
              );
            })}
          </div>

          {mode === 'restore' ? (
            <SetupRestoreForm defaultDbFolder={folder} pickFolder={pickFolder} onComplete={onComplete} />
          ) : (<>
          <ol aria-label="Setup progress" style={{ ...hybridEntryStyles.progress, listStyle: 'none', padding: 0 }}>
            <li aria-current="step" style={{ ...hybridEntryStyles.progressItem, ...hybridEntryStyles.progressItemActive }}>
              1. Choose storage
            </li>
            <li style={hybridEntryStyles.progressItem}>2. Create database</li>
            <li style={hybridEntryStyles.progressItem}>3. Sign in</li>
          </ol>

          <form onSubmit={handleSubmit} style={hybridEntryStyles.content}>
            <label
              htmlFor="setup-database-folder"
              style={{
                display: 'block',
                marginBottom: 8,
                color: 'var(--text)',
                fontSize: 14,
                fontWeight: 750,
              }}
            >
              Database folder
            </label>
            <div style={{ display: 'flex', gap: 8 }}>
              <input
                id="setup-database-folder"
                type="text"
                value={folder}
                onChange={event => {
                  userEditedFolderRef.current = true;
                  setFolder(event.target.value);
                }}
                placeholder={'e.g. C:\\BoogieBox\\data or \\\\server\\share\\boogieboxdb'}
                autoFocus
                disabled={loading}
                aria-describedby={`setup-folder-help${error ? ' setup-error' : ''}`}
                aria-invalid={Boolean(error)}
                style={{
                  ...hybridControlStyles.field,
                  flex: 1,
                  minWidth: 0,
                  boxSizing: 'border-box',
                  ...(error ? { borderColor: 'var(--danger)' } : {}),
                }}
              />
              <button
                type="button"
                onClick={() => void handleBrowse()}
                disabled={loading || browsing}
                style={{
                  ...hybridControlStyles.secondaryButton,
                  minWidth: 92,
                  ...(loading || browsing ? hybridControlStyles.disabled : {}),
                }}
              >
                {browsing ? 'Opening…' : 'Browse'}
              </button>
            </div>

            <div id="setup-folder-help" style={hybridEntryStyles.technicalNote}>
              Local and UNC network paths are supported. BoogieBox stores its database and
              settings here; it does not move your music.
            </div>

            {error && (
              <div id="setup-error" role="alert" style={hybridEntryStyles.error}>
                {error}
              </div>
            )}

            <button
              type="submit"
              disabled={setupDisabled}
              style={{
                ...hybridControlStyles.primaryButton,
                width: '100%',
                marginTop: 20,
                ...(setupDisabled ? hybridControlStyles.disabled : {}),
              }}
            >
              {loading ? 'Setting up…' : 'Set up BoogieBox'}
            </button>
          </form>
          </>)}

          <footer style={hybridEntryStyles.footer}>
            {mode === 'restore'
              ? 'One-time setup • Sign in with your existing user afterwards'
              : 'One-time setup • You can manage libraries after signing in'}
          </footer>
        </section>
      </main>
    </>
  );
}

const S: Record<string, React.CSSProperties> = {
  choices: { display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 10 },
  choice: {
    display: 'flex', gap: 12, alignItems: 'flex-start', padding: 14, borderRadius: 12,
    border: '1px solid var(--border)', background: 'var(--surface-raised, var(--surface))', cursor: 'pointer',
  },
  choiceActive: { borderColor: 'var(--accent)', background: 'var(--accent-soft, var(--surface))' },
};
