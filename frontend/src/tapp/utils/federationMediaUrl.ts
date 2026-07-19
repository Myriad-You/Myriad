/**
 * Pure helpers for federation Note media attachment URLs.
 *
 * Mirrors backend `validate_attachment_url` path shape:
 *   {origin}/media/federation/{userId}/{filename}
 * where filename is a single path segment of [A-Za-z0-9._-]+.
 *
 * Host/base enforcement stays on the server; the client rejects empty and
 * obviously wrong paths early so compose never half-publishes bad attachments.
 */

// \w = [A-Za-z0-9_]; also allow . and - in stored filenames
const FEDERATION_MEDIA_PATH = /^\/media\/federation\/(\d+)\/([\w.-]+)$/

export interface FederationMediaUrlParts {
  userId: number
  filename: string
  origin: string
}

/**
 * Returns true when `url` is a well-formed absolute federation media URL.
 */
export function isValidFederationMediaUrl(url: unknown): boolean {
  return parseFederationMediaUrl(url) !== null
}

/**
 * Parse a federation media URL; null if invalid.
 */
export function parseFederationMediaUrl(
  url: unknown,
): FederationMediaUrlParts | null {
  if (typeof url !== 'string') return null
  const trimmed = url.trim()
  if (!trimmed) return null

  // Reject path tricks in the raw string before URL normalization resolves ".."
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

/**
 * Short English reason for UI/logs when validation fails (not i18n — bridge/logs).
 */
export function federationMediaUrlRejectionReason(url: unknown): string | null {
  if (url == null || (typeof url === 'string' && !url.trim())) {
    return 'Attachment URL is empty'
  }
  if (typeof url !== 'string') {
    return 'Attachment URL must be a string'
  }
  if (parseFederationMediaUrl(url)) return null
  return 'Attachment URL must look like /media/federation/{userId}/{filename} on http(s)'
}
