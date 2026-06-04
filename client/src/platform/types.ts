/**
 * Defines Types behavior for BoogieBox.
 */

export type DesktopConfig = {
  serverUrl?: string | null;
};

/** Server Probe Result is part of this module's public API. */
export type ServerProbeResult = {
  reachable: boolean;
  url: string;
  version?: string | null;
  app?: string | null;
  setupRequired?: boolean | null;
  error?: string | null;
};

/** Server Discovery Result is part of this module's public API. */
export type ServerDiscoveryResult = {
  servers: ServerProbeResult[];
  scanned: number;
};

/**
 * Platform abstraction — capabilities that differ between browser, desktop (Tauri),
 * and mobile. Feature views should use this interface rather than branching on
 * `window.__TAURI__` or user-agent strings directly.
 */
export interface Platform {
  /** True when running inside the Tauri desktop shell */
  readonly isDesktop: boolean;

  /**
   * Read the persisted desktop configuration.
   * Returns null on the browser platform.
   */
  getConfig(): Promise<DesktopConfig | null>;

  /**
   * Probe the BoogieBox server at the given URL (or the configured URL).
   * Returns null on the browser platform.
   */
  probeServer(url?: string): Promise<ServerProbeResult | null>;

  /**
   * Discover running BoogieBox servers on localhost and the local network.
   * Returns null on the browser platform.
   */
  discoverServers(): Promise<ServerDiscoveryResult | null>;

  /**
   * Open a native folder picker.
   * Returns null on browser, cancel, or unsupported platforms.
   */
  selectFolder(initialDir?: string): Promise<string | null>;
}
