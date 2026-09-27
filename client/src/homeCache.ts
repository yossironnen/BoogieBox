/** Session-local Home snapshots. Never persisted across users or database switches. */
type Snapshot = { data?: unknown; loading: boolean };
const EMPTY: Snapshot = { loading: true };
const snapshots = new Map<string, Snapshot>();
const requests = new Map<string, Promise<void>>();
const listeners = new Set<() => void>();
const refreshers = new Set<() => void>();
let generation = 0;

/** Stable snapshots for React's external store subscription. */
export function homeSnapshot(key: string): Snapshot {
  return snapshots.get(key) ?? EMPTY;
}

/** Subscribes to changed Home snapshots. */
export function subscribeHome(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** Registers a mounted widget for refresh after a successful data mutation. */
export function onHomeInvalidated(refresh: () => void): () => void {
  refreshers.add(refresh);
  return () => { refreshers.delete(refresh); };
}

function publish(key: string, snapshot: Snapshot): void {
  // Home has a small fixed set of widgets; bound dynamic genre/tab keys too.
  if (!snapshots.has(key) && snapshots.size >= 64) {
    snapshots.delete(snapshots.keys().next().value!);
  }
  snapshots.set(key, snapshot);
  listeners.forEach(listener => listener());
}

/** Coalesces concurrent refreshes; preserves good data on transient errors. */
export function refreshHomeData<T>(key: string, load: () => Promise<T>): Promise<void> {
  const pending = requests.get(key);
  if (pending) return pending;
  const currentGeneration = generation;
  const previous = homeSnapshot(key);
  publish(key, { ...previous, loading: previous.data === undefined });
  const request = Promise.resolve().then(load).then(data => {
    if (generation === currentGeneration) publish(key, { data, loading: false });
  }).catch(() => {
    if (generation === currentGeneration) publish(key, { ...previous, loading: false });
  }).finally(() => {
    if (requests.get(key) === request) requests.delete(key);
  });
  requests.set(key, request);
  return request;
}

/** Retains visible snapshots while refreshing; clear on auth/database boundaries. */
export function invalidateHomeData(clear = false): void {
  generation += 1;
  requests.clear();
  if (clear) snapshots.clear();
  listeners.forEach(listener => listener());
  if (!clear) refreshers.forEach(refresh => refresh());
}

/** Applies a successful local mutation immediately while a server refresh is pending. */
export function updateHomeData<T>(key: string, update: (previous: T | undefined) => T): void {
  publish(key, { data: update(homeSnapshot(key).data as T | undefined), loading: false });
}
