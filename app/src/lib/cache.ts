// Tiny async memoization helper — used to make revisiting a texture (switching folders,
// going back to Library after AiCompare, etc.) instant instead of re-invoking the backend
// and re-decoding the image every time.

export function memoizeAsync<T>(store: Map<string, Promise<T>>, key: string, compute: () => Promise<T>): Promise<T> {
  const cached = store.get(key);
  if (cached) return cached;
  const promise = compute().catch((err) => {
    // Don't cache failures — a transient error (file locked, etc.) shouldn't permanently
    // poison the cache for that key.
    store.delete(key);
    throw err;
  });
  store.set(key, promise);
  return promise;
}
