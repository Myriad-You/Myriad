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
        content_md: htmlToMarkdown(html),
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
