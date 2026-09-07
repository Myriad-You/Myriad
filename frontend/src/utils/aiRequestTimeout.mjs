/** Floor for waiting on a model. Do not use the generic 30s HTTP timeout. */
export const AI_REQUEST_TIMEOUT_FLOOR_MS = 5 * 60 * 1000
/** Image / 3D / portrait sockets can stay idle until the provider returns. */
export const AI_IMAGE_REQUEST_TIMEOUT_MS = 15 * 60 * 1000

const IMAGE_AI_PREFIXES = [
  '/api/merope/',
  '/api/home/stickers',
  '/api/model3d/tasks',
  '/api/tapp/3d/',
  '/api/agent/persona/',
]

const LONG_AI_PREFIXES = [
  '/api/speech',
  '/api/tapp/ai/',
  '/api/tapp-playground/',
  '/api/reports/generate',
  '/api/reports/platform',
  '/api/prompt/generate',
  '/api/seo/generate-copy',
  '/api/ai/',
  '/api/brewlia',
  '/api/agent/process',
  '/api/agent/clarify',
]

export function requestPathname(urlPath) {
  const raw = String(urlPath || '').trim()
  try {
    const url =
      raw.startsWith('http://') || raw.startsWith('https://')
        ? new URL(raw)
        : new URL(raw, 'http://dev.invalid')
    return url.pathname.replace(/\/+$/, '') || '/'
  } catch {
    const path = (raw.split('?')[0] || '').split('#')[0].replace(/\/+$/, '')
    return path || '/'
  }
}

function matchesPrefix(path, prefix) {
  return path === prefix || path.startsWith(prefix)
}

/**
 * @param {string} url
 * @returns {number | undefined}
 */
export function aiRequestTimeoutMs(url) {
  const path = requestPathname(url)
  if (IMAGE_AI_PREFIXES.some((prefix) => matchesPrefix(path, prefix))) {
    return AI_IMAGE_REQUEST_TIMEOUT_MS
  }
  if (/^\/api\/agent\/tasks\/[^/]+\/answer/.test(path)) {
    return AI_REQUEST_TIMEOUT_FLOOR_MS
  }
  if (LONG_AI_PREFIXES.some((prefix) => matchesPrefix(path, prefix))) {
    return AI_REQUEST_TIMEOUT_FLOOR_MS
  }
  return undefined
}

/** Attach a floor timeout unless the caller already passed a signal. */
export function withAiTimeoutSignal(url, init = {}) {
  if (init.signal) return init
  const ms = aiRequestTimeoutMs(url)
  if (!ms) return init
  return { ...init, signal: AbortSignal.timeout(ms) }
}
