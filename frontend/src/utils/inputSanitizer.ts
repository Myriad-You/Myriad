export function escapeHtml(unsafe: string): string {
  return unsafe
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;')
}

export function sanitizeUsername(username: string): string {
  return username.replaceAll(/\W/g, '').slice(0, 50)
}

export function sanitizeUrl(url: string): string {
  try {
    const parsed = new URL(url)
    // http(s) only.
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error('Invalid protocol')
    }
    return parsed.toString()
  } catch {
    return ''
  }
}

export function sanitizeFilename(filename: string): string {
  return filename.replaceAll(/[<>:"/\\|?*\x00-\x1F]/g, '_').slice(0, 255)
}
