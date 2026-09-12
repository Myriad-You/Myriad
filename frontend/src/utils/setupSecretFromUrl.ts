const SETUP_SECRET_RE = /^[\w-]{32,512}$/
const PARAM = 'setup_secret'

export interface SetupSecretLocation {
  search: string
  hash: string
  pathname: string
}

function parseSearch(search: string): URLSearchParams {
  const raw = search.startsWith('?') ? search.slice(1) : search
  return new URLSearchParams(raw)
}

function parseHashParams(hash: string): URLSearchParams | null {
  if (!hash || hash === '#') return null
  const body = hash.startsWith('#') ? hash.slice(1) : hash
  if (!body || body.startsWith('/')) return null
  return new URLSearchParams(body)
}

function formatHash(params: URLSearchParams): string {
  const text = params.toString()
  return text ? `#${text}` : ''
}

export function consumeSetupSecretFromLocation(
  loc: SetupSecretLocation,
  replaceState?: (url: string) => void,
): string | null {
  const hashParams = parseHashParams(loc.hash)
  const queryParams = parseSearch(loc.search)
  const fromHash = (hashParams?.get(PARAM) || '').trim()
  const fromQuery = (queryParams.get(PARAM) || '').trim()
  const raw = fromHash || fromQuery
  const value = SETUP_SECRET_RE.test(raw) ? raw : null

  const hadQuery = queryParams.has(PARAM)
  const hadHash = Boolean(hashParams?.has(PARAM))
  if (hadQuery || hadHash) {
    queryParams.delete(PARAM)
    if (hashParams) hashParams.delete(PARAM)
    const qs = queryParams.toString()
    const nextHash = hashParams ? formatHash(hashParams) : loc.hash
    replaceState?.(loc.pathname + (qs ? `?${qs}` : '') + nextHash)
  }

  return value
}
