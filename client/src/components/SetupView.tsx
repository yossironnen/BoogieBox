/**
 * Defines the Setup View React component and related UI helpers.
 */

import React, { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { hybridControlStyles, hybridEntryStyles } from '../hybridPreview';
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

  const handleBrowse = async () => {
    if (platform.isDesktop) {
      setBrowsing(true);
      setError(null);
      try {
        const selected = await platform.selectFolder(folder.trim() || DEFAULT_DATABASE_FOLDER);
        if (selected) {
          userEditedFolderRef.current = true;
          setFolder(selected);
        }
      } catch (pickerError: any) {
        setError(pickerError.message || 'Folder picker failed');
      } finally {
        setBrowsing(false);
      }
      return;
    }

    const selected = await new Promise<string | null>(resolve => {
      pickerResolveRef.current = resolve;
      setPickerInitial(folder.trim() || undefined);
      setPickerOpen(true);
    });
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
            <p style={hybridEntryStyles.description}>
              Choose where BoogieBox will keep its database. Your music stays in its existing
              folders.
            </p>
          </header>

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

          <footer style={hybridEntryStyles.footer}>
            One-time setup • You can manage libraries after signing in
          </footer>
        </section>
      </main>
    </>
  );
}
