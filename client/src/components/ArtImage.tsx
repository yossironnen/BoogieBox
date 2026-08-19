/**
 * Defines the Art Image React component and related UI helpers.
 */

import React, { useEffect, useRef, useState } from 'react';

/** Art Image Load State is part of this module's public API. */
export type ArtImageLoadState = 'idle' | 'loading' | 'loaded' | 'error';

type ArtImageProps = {
  src: string | null;
  alt: string;
  imgStyle: React.CSSProperties;
  wrapperStyle?: React.CSSProperties;
  fallback?: React.ReactNode;
  eager?: boolean;
  fetchPriority?: 'high' | 'low' | 'auto';
  onLoadStateChange?: (state: ArtImageLoadState) => void;
  onImageReady?: (img: HTMLImageElement | null) => void;
};

/**
 * Art Image is part of this module's public API.
 *
 * Deferral is delegated entirely to the native `loading="lazy"` attribute rather than a
 * manual IntersectionObserver: the browser's own lazy-image scheduler is designed to work
 * alongside `content-visibility: auto` ancestors (as used by Browse's virtualized rows/grid
 * tiles), whereas a nested IntersectionObserver watching a span inside a `content-visibility:
 * auto` subtree may not fire promptly — that subtree isn't laid out until the ancestor itself
 * decides it's near-viewport, so a second, JS-driven lazy gate on top just adds latency before
 * the image starts loading.
 */
export default function ArtImage({
  src,
  alt,
  imgStyle,
  wrapperStyle,
  fallback = null,
  eager = false,
  fetchPriority,
  onLoadStateChange,
  onImageReady,
}: ArtImageProps) {
  const imgRef = useRef<HTMLImageElement | null>(null);
  const [loadState, setLoadState] = useState<ArtImageLoadState>(src ? 'loading' : 'error');
  const imgProps = fetchPriority ? ({ fetchpriority: fetchPriority } as Record<string, string>) : null;

  useEffect(() => {
    setLoadState(src ? 'loading' : 'error');
  }, [src]);

  useEffect(() => {
    onLoadStateChange?.(loadState);
  }, [loadState, onLoadStateChange]);

  useEffect(() => {
    onImageReady?.(imgRef.current);
  }, [loadState, onImageReady]);

  return (
    <span style={{ display: 'block', ...wrapperStyle }}>
      {src ? (
        <img
          ref={imgRef}
          src={src}
          alt={alt}
          loading={eager ? 'eager' : 'lazy'}
          decoding="async"
          {...imgProps}
          style={imgStyle}
          onLoad={() => setLoadState('loaded')}
          onError={() => {
            onImageReady?.(null);
            setLoadState('error');
          }}
        />
      ) : null}
      {(!src || loadState === 'error') ? fallback : null}
    </span>
  );
}
