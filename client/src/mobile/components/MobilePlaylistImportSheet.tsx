/**
 * Mobile "Import playlist" sheet: the same preview as the desktop dialog, laid out for a phone.
 */

import React from 'react';
import MobileBottomSheet from './MobileBottomSheet';
import {
  CreatePlaylistButton,
  ImportWillAdd,
  PlaylistImportBody,
  usePlaylistImport,
  type ImportedPlaylist,
} from '../../components/PlaylistImport';

export default function MobilePlaylistImportSheet({ file, onClose, onImported }: {
  file: File;
  onClose: () => void;
  onImported: (result: ImportedPlaylist) => void;
}) {
  const state = usePlaylistImport(file);
  return (
    <MobileBottomSheet title="Import playlist" closeLabel="Close import" onClose={onClose}>
      <div style={styles.body}>
        <PlaylistImportBody state={state} compact />
      </div>
      {state.phase !== 'problem' && (
        <div style={styles.footer}>
          <ImportWillAdd state={state} center />
          <CreatePlaylistButton state={state} onCreated={onImported} style={styles.create} />
        </div>
      )}
    </MobileBottomSheet>
  );
}

const styles: Record<string, React.CSSProperties> = {
  body: { padding: '4px 16px 16px' },
  footer: { padding: '12px 16px 8px', borderTop: '1px solid var(--border)', display: 'flex', flexDirection: 'column', gap: 10 },
  create: { minHeight: 48, borderRadius: 12, fontSize: 16 },
};
