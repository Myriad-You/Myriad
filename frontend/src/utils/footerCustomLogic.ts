/**
 * 页脚自定义项：解析 / 规范化 / 序列化（最多 2 条）
 * 文案入库与展示前会剥 HTML 标签，避免误存脚本片段。
 */

export const FOOTER_CUSTOM_MAX = 2
/** 单条文案长度上限（剥标签后） */
export const FOOTER_CUSTOM_TEXT_MAX = 64

export interface FooterCustomItem {
  /** 展示文案（有文案才在页脚渲染） */
  text: string
  /** 图标 URL 或 data:image */
  icon: string
  /** 可选跳转链接 */
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

/**
 * 剥掉 HTML 标签与常见实体，保留纯文本。
 * 用于页脚自定义文案；不依赖 DOM（可在 node 测试里跑）。
 */
export function stripHtmlTags(input: string): string {
  if (!input) return ''
  let s = input
  // 去掉 script/style 整块（含内容）
  s = s.replace(/<\s*(script|style)[^>]*>[\s\S]*?<\s*\/\s*\1\s*>/gi, '')
  // 去掉所有标签
  s = s.replace(/<[^>]*>/g, '')
  // 解码常见命名实体与数字实体
  s = s.replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (full, body: string) => {
    const key = body.toLowerCase()
    if (key in ENTITY_MAP) return ENTITY_MAP[key]!
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
  // 控制字符与多余空白
  s = s.replace(/[\u0000-\u0008\v\f\u000E-\u001F\u007F]/g, '')
  s = s.replace(/\s+/g, ' ').trim()
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

/**
 * 编辑器用：保留空槽（便于「添加」后继续填），最多 {@link FOOTER_CUSTOM_MAX} 条。
 */
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

/**
 * 页脚渲染用：仅有文案的项；非法时返回 []。
 */
export function parseFooterCustom(
  raw: string | null | undefined,
): FooterCustomItem[] {
  return parseFooterCustomSlots(raw).filter((it) => it.text)
}

/**
 * 序列化编辑槽（含尚未填字的空槽，保证「添加」可 round-trip）。
 * 长度 0 → 空串；全是空字段的槽仍写入，方便表单编辑。
 * 页脚渲染侧用 {@link parseFooterCustom} 会丢掉无 text 的项。
 */
export function serializeFooterCustom(items: FooterCustomItem[]): string {
  const cleaned = items.slice(0, FOOTER_CUSTOM_MAX).map((it) => ({
    text: sanitizeFooterText(it.text || ''),
    icon: (it.icon || '').trim(),
    url: (it.url || '').trim(),
  }))
  if (cleaned.length === 0) return ''
  return JSON.stringify(cleaned)
}

/** 是否可作为外链（相对路径、http(s)、mailto） */
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
