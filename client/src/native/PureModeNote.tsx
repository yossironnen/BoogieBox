/**
 * In the desktop app, while Pure mode is on: says that a setting (crossfade, volume levelling) is off for the app's
 * own output. Copy: `wip/native-hires-approved-copy.md` § Phase 5.
 */

import React from 'react';
import { InfoCircleIcon } from './NativeIcons';
import { pureModeOn } from './nativeTypes';
import { useNativeAudio } from './useNativeAudio';

export default function PureModeNote({ style }: { style?: React.CSSProperties }) {
  const native = useNativeAudio();
  if (!native.available || !native.settings || !pureModeOn(native.settings)) return null;
  return (
    <div role="note" style={{ display: 'flex', alignItems: 'flex-start', gap: 6, fontSize: 12, color: 'var(--text-muted)', ...style }}>
      <InfoCircleIcon style={{ color: 'var(--accent)', opacity: 0.85 }} />
      Off for This PC while Pure mode is on.
    </div>
  );
}
