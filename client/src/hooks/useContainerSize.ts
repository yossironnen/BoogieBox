import { useCallback, useLayoutEffect, useState } from 'react';
import type React from 'react';

/** Measures a scroll container's `clientWidth`/`clientHeight` (padding-box,
 * matching the CSS `padding` on the same element), synchronously as soon as
 * the element is attached (before paint, so real layouts never flash a full
 * unwindowed render first) and on every resize thereafter. Returns `{0, 0}`
 * until a real measurement is available.
 *
 * Takes an existing stable ref object (kept for every other imperative use
 * already in this file — scroll manipulation, event listeners, etc.) and
 * returns an `attachRef` callback to place on the JSX element *instead of*
 * that ref directly. This is not cosmetic: `AlbumGrid`/`AlbumList` both have
 * an early return (`if (!albums.length) return <div>No albums found</div>`)
 * *after* their hooks — so on first mount, before the root Browse fetch has
 * resolved, `albums` is still `[]` and the real scroll container never
 * renders at all that pass. A plain `useLayoutEffect(..., [ref])` already
 * ran once against `ref.current === null` on that pass and, since a `useRef`
 * object's identity never changes, never runs again — so once the container
 * *does* mount (album data arrives, a later render takes the other branch),
 * nothing ever measures it and windowing is stuck computing from `{0, 0}`
 * forever. Routing attachment through `useState` instead makes "the element
 * showed up" a real dependency change the effect below reacts to. */
export function useContainerSize(stableRef: React.RefObject<HTMLElement | null>): {
  size: { width: number; height: number };
  attachRef: (el: HTMLElement | null) => void;
} {
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [node, setNode] = useState<HTMLElement | null>(null);

  const attachRef = useCallback((el: HTMLElement | null) => {
    stableRef.current = el;
    setNode(el);
  }, [stableRef]);

  useLayoutEffect(() => {
    if (!node) return;
    const measure = () => {
      setSize((prev) => {
        const width = node.clientWidth;
        const height = node.clientHeight;
        return prev.width === width && prev.height === height ? prev : { width, height };
      });
    };
    measure();
    // Belt-and-suspenders: the synchronous measurement above can still
    // occasionally run before an ancestor's layout has fully settled. A
    // one-time re-check on the next animation frame self-heals that.
    const raf = requestAnimationFrame(measure);
    if (typeof ResizeObserver === 'undefined') {
      return () => cancelAnimationFrame(raf);
    }
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
  }, [node]);

  return { size, attachRef };
}
