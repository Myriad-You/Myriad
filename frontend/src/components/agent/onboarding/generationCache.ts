const PREFIX = 'myriad_persona_ob_'

export function generationCacheKey(kind: string, parts: string[]): string {
  return `${PREFIX}${kind}:${parts.join('|')}`
}

export function getGenerationCache<T>(key: string): T | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = sessionStorage.getItem(key)
    if (!raw) return null
    return JSON.parse(raw) as T
  } catch {
    return null
  }
}

export function setGenerationCache<T>(key: string, value: T) {
  if (typeof window === 'undefined') return
  try {
    sessionStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* ignore */
  }
}
