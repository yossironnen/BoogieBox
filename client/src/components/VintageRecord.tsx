/**
 * Vintage (Record Shop) record disc that peeks out from behind an album sleeve.
 * Its centre label is a round crop of the album cover. Renders nothing outside
 * a Vintage style. Position it inside a `position: relative` sleeve box and put
 * the cover in front with VINTAGE_SLEEVE_COVER_STYLE.
 */

import React from 'react';
import { api } from '../api';
import type { ClientEntityId } from '../types';
import { useVintageStyle } from '../vintageThemes';
import ArtImage from './ArtImage';

/** The cover sits in front of the record (shared by Home and Browse). */
export const VINTAGE_SLEEVE_COVER_STYLE: React.CSSProperties = {
  position: 'relative',
  zIndex: 1,
  boxShadow: '3px 3px 0 rgba(43,33,24,0.25)',
};

const LABEL_IMG_STYLE: React.CSSProperties = {
  width: '100%',
  height: '100%',
  objectFit: 'cover',
  display: 'block',
};

/** Vintage Record is part of this module's public API. */
export default function VintageRecord({
  albumId,
  hovered = false,
  hoverShift,
}: {
  albumId: ClientEntityId;
  hovered?: boolean;
  /** Overrides the style's hover slide (e.g. a smaller shift inside a tight grid tile). */
  hoverShift?: string;
}) {
  const vintage = useVintageStyle();
  if (!vintage) return null;
  const hoverStyle = hoverShift ? { transform: `translateX(${hoverShift})` } : vintage.styles.recentAlbumRecordHovered;
  return (
    <div
      data-vintage-record={albumId}
      aria-hidden="true"
      style={{
        ...vintage.styles.recentAlbumRecord,
        ...(hovered ? hoverStyle : {}),
      }}
    >
      <div data-vintage-record-label style={vintage.styles.recentAlbumLabel}>
        <ArtImage src={api.albumArtUrl(albumId, 300)} alt="" imgStyle={LABEL_IMG_STYLE} />
        <span style={vintage.styles.recentAlbumSpindle} />
      </div>
    </div>
  );
}
