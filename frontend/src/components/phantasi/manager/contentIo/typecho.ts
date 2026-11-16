import type { TransferNote } from './types'
import { addressFields, importOldPaths, importSlug } from './addresses'
import { escapeXml, htmlToMarkdown, parseUnixOrDate, xmlBlocks, xmlInner } from './text'
import { parseWordpressXml } from './wordpress'

export function parseTypechoXml(xml: string): TransferNote[] {
  if (/<wp:wxr_version[\s>]|xmlns:wp=/i.test(xml)) {
    return parseWordpressXml(xml)
  }
  const items = xmlBlocks(xml, 'item')
  return items
    .map((item) => {
      const type = (xmlInner(item, 'type') || 'post').toLowerCase()
      if (type && type !== 'post' && type !== 'page') return null
      const text = xmlInner(item, 'text') || xmlInner(item, 'content')
      const looksHtml = /<\/?[a-z][\s\S]*>/i.test(text)
      const note: TransferNote = {
        title: xmlInner(item, 'title'),
        content_md: looksHtml ? htmlToMarkdown(text) : text,
        topic:
          xmlInner(item, 'categories') || xmlInner(item, 'category') || null,
        published_at: parseUnixOrDate(
          xmlInner(item, 'created') || xmlInner(item, 'modified'),
        ),
        status:
          xmlInner(item, 'status').toLowerCase() === 'publish'
            ? 'published'
            : 'draft',
        // 备份里带了文章地址就当旧地址；只有 slug 时拼不出地址（路由规则在旧站配置里）。
        ...addressFields(
          importSlug(xmlInner(item, 'slug')),
          importOldPaths([xmlInner(item, 'permalink'), xmlInner(item, 'link')]),
        ),
      }
      return note
    })
    .filter(
      (note): note is TransferNote =>
        note != null &&
        (note.title.trim() !== '' || note.content_md.trim() !== ''),
    )
}

export function serializeTypechoXml(notes: readonly TransferNote[]): string {
  const items = notes
    .map((note, index) => {
      const created = note.published_at ?? Math.round(Date.now() / 1000)
      const topic = note.topic
        ? `\n    <categories><![CDATA[${note.topic}]]></categories>`
        : ''
      const slug = note.slug ? `\n    <slug>${escapeXml(note.slug)}</slug>` : ''
      return `  <item>
    <cid>${index + 1}</cid>
    <title><![CDATA[${note.title || `note-${index + 1}`}]]></title>
    <created>${created}</created>
    <text><![CDATA[${note.content_md}]]></text>
    <type>post</type>
    <status>${note.status === 'published' ? 'publish' : 'draft'}</status>${topic}${slug}
  </item>`
    })
    .join('\n')

  return `<?xml version="1.0" encoding="UTF-8"?>
<typecho version="1.2">
<contents>
${items}
</contents>
</typecho>
`
}
