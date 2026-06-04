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
  rootMargin?: string;
  fetchPriority?: 'high' | 'low' | 'auto';
  onLoadStateChange?: (state: ArtImageLoadState) => void;
  onImageReady?: (img: HTMLImageElement | null) => void;
};

/** Art Image is part of this module's public API. */
export default function ArtImage({
  src,
  alt,
  imgStyle,
  wrapperStyle,
  fallback = null,
  eager = false,
  rootMargin = '160px',
  fetchPriority,
  onLoadStateChange,
  onImageReady,
}: ArtImageProps) {
  const hostRef = useRef<HTMLSpanElement | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const [enabled, setEnabled] = useState(eager || !src || typeof window === 'undefined' || typeof window.IntersectionObserver === 'undefined');
  const [loadState, setLoadState] = useState<ArtImageLoadState>(src ? (enabled ? 'loading' : 'idle') : 'error');
  const imgProps = fetchPriority ? ({ fetchpriority: fetchPriority } as Record<string, string>) : null;

  useEffect(() => {
    const immediate = eager || !src || typeof window === 'undefined' || typeof window.IntersectionObserver === 'undefined';
    setEnabled(immediate);
    setLoadState(src ? (immediate ? 'loading' : 'idle') : 'error');
  }, [eager, src]);

  useEffect(() => {
    if (enabled || !src) return;
    const node = hostRef.current;
    if (!node || typeof window === 'undefined' || typeof window.IntersectionObserver === 'undefined') return;
    const observer = new window.IntersectionObserver(
      ([entry]) => {
        if (!entry.isIntersecting) return;
        observer.disconnect();
        setEnabled(true);
        setLoadState('loading');
      },
      { rootMargin },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [enabled, rootMargin, src]);

  useEffect(() => {
    onLoadStateChange?.(loadState);
  }, [loadState, onLoadStateChange]);

  useEffect(() => {
    onImageReady?.(imgRef.current);
  }, [loadState, onImageReady]);

  const activeSrc = src && enabled ? src : null;

  return (
    <span ref={hostRef} style={{ display: 'block', ...wrapperStyle }}>
      {activeSrc ? (
        <img
          ref={imgRef}
          src={activeSrc}
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
      {(!activeSrc || loadState === 'error') ? fallback : null}
    </span>
  );
}
