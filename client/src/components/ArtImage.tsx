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

// Bounded retry for a cache miss whose fill is running in the background
// (plan Phase 3.2 §8.2 item 4): the server returns a fast, non-cacheable
// "not yet available" response while it fills the art off the request path,
// so a plain `<img>` (no visibility into response status/headers) just
// retries blindly a small fixed number of times with backoff, appending a
// cache-busting param each time so the browser/proxy never serves a cached
// miss back. A genuinely absent image (no folder art, no provider result)
// fails the same way every retry — negative-cached server-side, so those
// retries stay cheap — and falls back once attempts are exhausted.
const ART_RETRY_DELAYS_MS = [400, 800, 1600, 3200];

function withCacheBust(src: string, attempt: number): string {
  const sep = src.includes('?') ? '&' : '?';
  return `${src}${sep}_retry=${attempt}`;
}

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
  const [retryAttempt, setRetryAttempt] = useState(0);
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearPendingRetry = () => {
    if (retryTimerRef.current != null) {
      clearTimeout(retryTimerRef.current);
      retryTimerRef.current = null;
    }
  };

  useEffect(() => {
    setLoadState(src ? 'loading' : 'error');
    setRetryAttempt(0);
    clearPendingRetry();
    // Deliberately keyed on `src` alone: this must reset only on a genuine
    // source change, not on our own retry re-renders.
  }, [src]);

  useEffect(() => clearPendingRetry, []);

  useEffect(() => {
    onLoadStateChange?.(loadState);
  }, [loadState, onLoadStateChange]);

  useEffect(() => {
    onImageReady?.(imgRef.current);
  }, [loadState, onImageReady]);

  const resolvedSrc = src && retryAttempt > 0 ? withCacheBust(src, retryAttempt) : src;

  return (
    <span style={{ display: 'block', ...wrapperStyle }}>
      {src ? (
        <img
          ref={imgRef}
          src={resolvedSrc ?? undefined}
          alt={alt}
          loading={eager ? 'eager' : 'lazy'}
          decoding="async"
          fetchPriority={fetchPriority}
          style={imgStyle}
          onLoad={() => setLoadState('loaded')}
          onError={() => {
            onImageReady?.(null);
            if (retryAttempt < ART_RETRY_DELAYS_MS.length) {
              const delay = ART_RETRY_DELAYS_MS[retryAttempt];
              clearPendingRetry();
              retryTimerRef.current = setTimeout(() => {
                retryTimerRef.current = null;
                setRetryAttempt((n) => n + 1);
              }, delay);
            } else {
              setLoadState('error');
            }
          }}
        />
      ) : null}
      {(!src || loadState === 'error') ? fallback : null}
    </span>
  );
}
