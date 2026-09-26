/** 导入导出共用的文本收拾，不碰 DOM。 */

/** `&amp;` 必须最后解，否则 `&amp;lt;` 会被解两次变成 `<`。 */
export function decodeEntities(text: string): string {
  return text
    .replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, '$1')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&#x([0-9a-f]+);/gi, (_, hex: string) =>
      String.fromCodePoint(Number.parseInt(hex, 16)),
    )
    .replace(/&#(\d+);/g, (_, dec: string) => String.fromCodePoint(Number(dec)))
    .replace(/&amp;/g, '&')
}

export function xmlBlocks(xml: string, tag: string): string[] {
  const escaped = tag.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(
    `<${escaped}(?:\\s[^>]*)?>[\\s\\S]*?</${escaped}>`,
    'gi',
  )
  return xml.match(re) ?? []
}

export function xmlInner(block: string, tag: string): string {
  const escaped = tag.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(
    `<${escaped}(?:\\s[^>]*)?>([\\s\\S]*?)</${escaped}>`,
    'i',
  )
  const match = re.exec(block)
  return match ? decodeEntities(match[1]!.trim()) : ''
}

export function stripTags(html: string): string {
  return decodeEntities(html.replace(/<[^>]+>/g, ''))
}

export function htmlToMarkdown(html: string): string {
  let text = html.replaceAll('\r\n', '\n')
  text = text.replace(/<script[\s\S]*?<\/script>/gi, '')
  text = text.replace(/<style[\s\S]*?<\/style>/gi, '')
  text = text.replace(/<!--[\s\S]*?-->/g, '')
  text = text.replace(
    /<h([1-6])[^>]*>([\s\S]*?)<\/h\1>/gi,
    (_, level, inner) => {
      return `\n${'#'.repeat(Number(level))} ${stripTags(inner).trim()}\n`
    },
  )
  text = text.replace(
    /<img[^>]*alt="([^"]*)"[^>]*src="([^"]+)"[^>]*>/gi,
    '![$1]($2)',
  )
  text = text.replace(
    /<img[^>]*src="([^"]+)"[^>]*alt="([^"]*)"[^>]*>/gi,
    '![$2]($1)',
  )
  text = text.replace(/<img[^>]*src="([^"]+)"[^>]*>/gi, '![]($1)')
  text = text.replace(
    /<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/gi,
    (_, href, inner) => `[${stripTags(inner).trim() || href}](${href})`,
  )
  text = text.replace(/<(strong|b)[^>]*>([\s\S]*?)<\/\1>/gi, '**$2**')
  text = text.replace(/<(em|i)[^>]*>([\s\S]*?)<\/\1>/gi, '*$2*')
  text = text.replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, '`$1`')
  text = text.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_, inner) => {
    return `\n\`\`\`\n${stripTags(inner)}\n\`\`\`\n`
  })
  text = text.replace(
    /<blockquote[^>]*>([\s\S]*?)<\/blockquote>/gi,
    (_, inner) => {
      const body = htmlToMarkdown(inner)
      return `\n${body
        .split('\n')
        .map((line) => `> ${line}`)
        .join('\n')}\n`
    },
  )
  text = text.replace(/<li[^>]*>([\s\S]*?)<\/li>/gi, (_, inner) => {
    return `- ${htmlToMarkdown(inner).replaceAll('\n', ' ').trim()}\n`
  })
  text = text.replace(/<br\s*\/?>/gi, '\n')
  text = text.replace(/<\/(p|div|tr)>/gi, '\n')
  text = text.replace(/<[^>]+>/g, '')
  return decodeEntities(text)
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

export function markdownToHtml(markdown: string): string {
  const escaped = markdown
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
  const blocks = escaped.split(/\n{2,}/)
  return blocks
    .map((block) => {
      const line = block.trim()
      if (!line) return ''
      const heading = /^(#{1,6}) ([^\n]+)$/.exec(line)
      if (heading) {
        const level = heading[1]!.length
        return `<h${level}>${inlineMd(heading[2]!)}</h${level}>`
      }
      if (line.startsWith('```')) {
        const body = line.replace(/^```[^\n]*\n?/, '').replace(/```$/, '')
        return `<pre><code>${body}</code></pre>`
      }
      const html = inlineMd(line).replaceAll('\n', '<br>')
      return `<p>${html}</p>`
    })
    .filter(Boolean)
    .join('\n')
}

function inlineMd(text: string): string {
  return text
    .replace(/!\[([^\]]*)\]\(([^)]+)\)/g, '<img alt="$1" src="$2">')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\*([^*]+)\*/g, '<em>$1</em>')
    .replace(/`([^`]+)`/g, '<code>$1</code>')
}

export function parseUnixOrDate(
  value: string | number | null | undefined,
): number | null {
  if (value == null || value === '') return null
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value > 1e12 ? Math.round(value / 1000) : Math.round(value)
  }
  const raw = String(value).trim()
  if (/^\d+$/.test(raw)) {
    const n = Number(raw)
    return n > 1e12 ? Math.round(n / 1000) : n
  }
  const ms = Date.parse(raw.includes('T') ? raw : `${raw.replace(' ', 'T')}Z`)
  return Number.isFinite(ms) ? Math.round(ms / 1000) : null
}

export function fileSlug(title: string, index: number): string {
  const base = title
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)
  return base || `note-${index + 1}`
}

export function escapeXml(text: string): string {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
}
