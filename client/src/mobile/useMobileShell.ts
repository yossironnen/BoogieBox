/**
 * Defines mobile Use Mobile Shell behavior for the BoogieBox React client.
 */

import { useEffect, useState } from 'react';

/** Should Use Mobile Shell is part of this module's public API. */
export function shouldUseMobileShell(target: Window = window): boolean {
  const width = target.innerWidth;
  const height = target.innerHeight;
  const ua = target.navigator.userAgent.toLowerCase();
  const coarse = typeof target.matchMedia === 'function' && target.matchMedia('(pointer: coarse)').matches;
  const iphoneLike = /iphone|ipod|mobile/.test(ua);
  return width <= 430 || (iphoneLike && width <= 932) || (coarse && width <= 640 && height <= 1100);
}

/** Use Mobile Shell is part of this module's public API. */
export function useMobileShell(): boolean {
  const [isMobile, setIsMobile] = useState(() => (typeof window !== 'undefined' ? shouldUseMobileShell(window) : false));

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const sync = () => setIsMobile(shouldUseMobileShell(window));
    sync();
    window.addEventListener('resize', sync);
    window.addEventListener('orientationchange', sync);
    return () => {
      window.removeEventListener('resize', sync);
      window.removeEventListener('orientationchange', sync);
    };
  }, []);

  return isMobile;
}
