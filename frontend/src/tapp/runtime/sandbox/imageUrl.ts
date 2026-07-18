/**
 * Convert host-provided remote images into the same-origin image proxy URL that
 * the Tapp iframe CSP permits. Tapp code never receives a directly loadable
 * third-party URL, so images work without widening the sandbox policy.
 */
export function toSandboxImageUrl(
  value: unknown,
  hostOrigin = typeof window !== 'undefined' ? window.location.origin : '',
): string {
  if (typeof value !== 'string') return ''

  const url = value.trim()
  if (!url) return ''
  if (!/^https?:\/\//i.test(url)) return url

  if (hostOrigin) {
    try {
      if (new URL(url).origin === hostOrigin) return url
    } catch {
      return ''
    }
  }

  return `/api/proxy/image?url=${encodeURIComponent(url)}`
}
