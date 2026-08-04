/**
 * Node 24 LTS does not enable Web Storage by default (Node 25+ does).
 * Call from tests that touch sessionStorage so CI can stay on LTS without
 * --experimental-webstorage.
 */
export function ensureSessionStoragePolyfill(): void {
  if (typeof globalThis.sessionStorage !== 'undefined')
    return

  const store = new Map<string, string>()
  globalThis.sessionStorage = {
    get length() {
      return store.size
    },
    clear() {
      store.clear()
    },
    getItem(key: string) {
      return store.has(key) ? store.get(key)! : null
    },
    key(index: number) {
      return [...store.keys()][index] ?? null
    },
    removeItem(key: string) {
      store.delete(key)
    },
    setItem(key: string, value: string) {
      store.set(key, String(value))
    },
  } as Storage
}
