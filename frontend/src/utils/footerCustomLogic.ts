export const FOOTER_CUSTOM_MAX = 2
export const FOOTER_CUSTOM_TEXT_MAX = 64

export interface FooterCustomItem {
  text: string
  /** URL or data:image/*. */
  icon: string
  url: string
}

export function emptyFooterCustomItem(): FooterCustomItem {
  return { text: '', icon: '', url: '' }
}

const ENTITY_MAP: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
}

export function stripHtmlTags(input: string): string {
  if (!input) return ''
  let s = input
  s = s.replaceAll(/<\s*(script|style)[^>]*>[\s\S]*?<\s*\/\s*\1\s*>/gi, '')
  s = s.replaceAll(/<[^>]*>/g, '')
  s = s.replaceAll(/&(#x?[0-9a-f]+|[a-z]+);/gi, (full, body: string) => {
    const key = body.toLowerCase()
    if (Object.hasOwn(ENTITY_MAP, key)) return ENTITY_MAP[key]!
    if (key.startsWith('#x')) {
      const code = Number.parseInt(key.slice(2), 16)
      return Number.isFinite(code) ? String.fromCodePoint(code) : ''
    }
    if (key.startsWith('#')) {
      const code = Number.parseInt(key.slice(1), 10)
      return Number.isFinite(code) ? String.fromCodePoint(code) : ''
    }
    return full
  })
  s = s.replaceAll(/[\u0000-\u0008\v\f\u000E-\u001F\u007F]/g, '')
  s = s.replaceAll(/\s+/g, ' ').trim()
  return s
}

function sanitizeFooterText(raw: string): string {
  const plain = stripHtmlTags(raw)
  if (plain.length <= FOOTER_CUSTOM_TEXT_MAX) return plain
  return plain.slice(0, FOOTER_CUSTOM_TEXT_MAX)
}

function normalizeItem(entry: unknown): FooterCustomItem | null {
  if (!entry || typeof entry !== 'object') return null
  const rec = entry as Record<string, unknown>
  return {
    text: typeof rec.text === 'string' ? sanitizeFooterText(rec.text) : '',
    icon: typeof rec.icon === 'string' ? rec.icon.trim() : '',
    url: typeof rec.url === 'string' ? rec.url.trim() : '',
  }
}

export function parseFooterCustomSlots(
  raw: string | null | undefined,
): FooterCustomItem[] {
  if (!raw || !raw.trim()) return []
  try {
    const data = JSON.parse(raw) as unknown
    if (!Array.isArray(data)) return []
    const out: FooterCustomItem[] = []
    for (const entry of data) {
      const item = normalizeItem(entry)
      if (!item) continue
      out.push(item)
      if (out.length >= FOOTER_CUSTOM_MAX) break
    }
    return out
  } catch {
    return []
  }
}

export function parseFooterCustom(
  raw: string | null | undefined,
): FooterCustomItem[] {
  return parseFooterCustomSlots(raw).filter((it) => it.text)
}

export function serializeFooterCustom(items: FooterCustomItem[]): string {
  const cleaned = items.slice(0, FOOTER_CUSTOM_MAX).map((it) => ({
    text: sanitizeFooterText(it.text || ''),
    icon: (it.icon || '').trim(),
    url: (it.url || '').trim(),
  }))
  if (cleaned.length === 0) return ''
  return JSON.stringify(cleaned)
}

export function isFooterCustomHref(url: string): boolean {
  if (!url) return false
  if (url.startsWith('/') || url.startsWith('#')) return true
  try {
    const u = new URL(url)
    return (
      u.protocol === 'https:' ||
      u.protocol === 'http:' ||
      u.protocol === 'mailto:'
    )
  } catch {
    return false
  }
}
