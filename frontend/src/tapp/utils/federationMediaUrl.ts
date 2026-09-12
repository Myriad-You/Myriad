const FEDERATION_MEDIA_PATH = /^\/media\/federation\/(\d+)\/([\w.-]+)$/

export interface FederationMediaUrlParts {
  userId: number
  filename: string
  origin: string
}

export function isValidFederationMediaUrl(url: unknown): boolean {
  return parseFederationMediaUrl(url) !== null
}

export function parseFederationMediaUrl(
  url: unknown,
): FederationMediaUrlParts | null {
  if (typeof url !== 'string') return null
  const trimmed = url.trim()
  if (!trimmed) return null

  if (trimmed.includes('..')) return null

  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return null
  }

  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return null
  }

  if (parsed.pathname.includes('..')) return null

  const match = FEDERATION_MEDIA_PATH.exec(parsed.pathname)
  if (!match) return null

  const userId = Number(match[1])
  const filename = match[2]
  if (!Number.isFinite(userId) || userId <= 0) return null
  if (!filename || filename === '.' || filename === '..') return null

  return {
    userId,
    filename,
    origin: parsed.origin,
  }
}

export function federationMediaUrlRejectionReason(url: unknown): string | null {
  if (parseFederationMediaUrl(url)) return null
  return 'Invalid attachment URL'
}
