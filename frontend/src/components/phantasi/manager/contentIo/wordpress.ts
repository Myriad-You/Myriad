import type { TransferNote } from './types'
import { addressFields, importOldPaths, importSlug, OLD_PATH_META_KEY } from './addresses'
import {
  escapeXml,
  htmlToMarkdown,
  markdownToHtml,
  parseUnixOrDate,
  xmlBlocks,
  xmlInner,
} from './text'

const SKIP_TYPES = new Set(['attachment', 'nav_menu_item', 'revision'])

/**
 * 旧站主题和插件的简码（`[showcard title="x"]内容[/showcard]`、`[gallery ids="1,2"]`）
 * 换了地方就没人认，原样留下只是一串方括号。成对的只留里面的内容，单个带参数的整个去掉；
 * 不带参数又不成对的（`[1]`、`[注]`）可能是正文，不动。`[[x]]` 是旧站里写出字面 `[x]` 的写法。
 */
export function stripShortcodes(html: string): string {
  const name = '([a-z][\\w-]*)'
  let out = html
  for (;;) {
    // 先拆最里层：内容里不再含同名的开头。
    const next = out.replace(
      new RegExp(`(?<!\\[)\\[${name}(?:\\s[^\\]]*)?\\]((?:(?!\\[\\1[\\s\\]])[\\s\\S])*?)\\[\\/\\1\\](?!\\])`, 'gi'),
      '$2',
    )
    if (next === out) break
    out = next
  }
  return out
    .replace(new RegExp(`(?<!\\[)\\[${name}\\s[^\\]=]*=[^\\]]*\\](?!\\])`, 'gi'), '')
    .replace(new RegExp(`(?<!\\[)\\[${name}\\s*\\/\\](?!\\])`, 'gi'), '')
    .replace(/\[\[([^[\]]+)\]\]/g, '[$1]')
}

export function parseWordpressXml(xml: string): TransferNote[] {
  return xmlBlocks(xml, 'item')
    .map((item) => {
      const type = xmlInner(item, 'wp:post_type') || 'post'
      if (SKIP_TYPES.has(type)) return null
      const statusRaw = xmlInner(item, 'wp:status').toLowerCase()
      const title = xmlInner(item, 'title')
      const html =
        xmlInner(item, 'content:encoded') || xmlInner(item, 'description')
      const topic = xmlInner(item, 'category') || null
      const published = parseUnixOrDate(
        xmlInner(item, 'wp:post_date_gmt') ||
          xmlInner(item, 'wp:post_date') ||
          xmlInner(item, 'pubDate'),
      )
      // <link> 是旧站上的文章地址；wp:post_name 是 URL 编码过的 slug。
      const oldPaths = importOldPaths([
        xmlInner(item, 'link'),
        ...xmlBlocks(item, 'wp:postmeta')
          .filter((meta) => xmlInner(meta, 'wp:meta_key') === OLD_PATH_META_KEY)
          .map((meta) => xmlInner(meta, 'wp:meta_value')),
      ])
      const note: TransferNote = {
        title,
        content_md: htmlToMarkdown(stripShortcodes(html)),
        topic: topic?.trim() || null,
        published_at: published,
        status: statusRaw === 'publish' ? 'published' : 'draft',
        ...addressFields(importSlug(xmlInner(item, 'wp:post_name')), oldPaths),
      }
      return note
    })
    .filter(
      (note): note is TransferNote =>
        note != null &&
        (note.title.trim() !== '' || note.content_md.trim() !== ''),
    )
}

export function serializeWordpressXml(
  notes: readonly TransferNote[],
  siteTitle: string,
): string {
  const items = notes
    .map((note, index) => {
      const date = note.published_at
        ? new Date(note.published_at * 1000)
            .toISOString()
            .slice(0, 19)
            .replace('T', ' ')
        : ''
      const topic = note.topic
        ? `\n    <category><![CDATA[${note.topic}]]></category>`
        : ''
      const slug = note.slug
        ? `\n    <wp:post_name>${escapeXml(note.slug)}</wp:post_name>`
        : ''
      const oldPaths = (note.old_paths ?? [])
        .map((path) => `
    <wp:postmeta>
      <wp:meta_key>${OLD_PATH_META_KEY}</wp:meta_key>
      <wp:meta_value>${escapeXml(path)}</wp:meta_value>
    </wp:postmeta>`)
        .join('')
      return `  <item>
    <title><![CDATA[${note.title || `note-${index + 1}`}]]></title>
    <content:encoded><![CDATA[${markdownToHtml(note.content_md)}]]></content:encoded>
    <wp:post_type>post</wp:post_type>
    <wp:status>${note.status === 'published' ? 'publish' : 'draft'}</wp:status>
    <wp:post_date_gmt>${escapeXml(date)}</wp:post_date_gmt>${topic}${slug}${oldPaths}
  </item>`
    })
    .join('\n')

  return `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"
  xmlns:excerpt="http://wordpress.org/export/1.2/excerpt/"
  xmlns:content="http://purl.org/rss/1.0/modules/content/"
  xmlns:wp="http://wordpress.org/export/1.2/">
<channel>
  <title><![CDATA[${siteTitle}]]></title>
  <wp:wxr_version>1.2</wp:wxr_version>
${items}
</channel>
</rss>
`
}
