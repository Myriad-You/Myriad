import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { parseTransferFile } from './contentIo.ts'
import { parseHaloJson, serializeHaloJson } from './halo.ts'
import { parseMarkdownFile, serializeMarkdownFile } from './markdown.ts'
import { fileSlug, htmlToMarkdown, markdownToHtml } from './text.ts'
import { parseTypechoXml, serializeTypechoXml } from './typecho.ts'
import { parseWordpressXml, serializeWordpressXml } from './wordpress.ts'

const sample = {
  title: '第一篇',
  content_md: '你好 **世界**\n\n这是正文。',
  topic: '日常',
  published_at: 1_700_000_000,
  status: 'published' as const,
}

describe('content Io text', () => {
  it('html 和 markdown 能对上标题、加粗和链接', () => {
    const md = htmlToMarkdown(
      '<h2>Hi</h2><p>Hello <strong>there</strong> and <a href="https://a.test">link</a></p>',
    )
    assert.match(md, /^## Hi/m)
    assert.match(md, /\*\*there\*\*/)
    assert.match(md, /\[link\]\(https:\/\/a\.test\)/)
    const html = markdownToHtml('## Hi\n\nHello **there**')
    assert.match(html, /<h2>Hi<\/h2>/)
    assert.match(html, /<strong>there<\/strong>/)
  })

  it('实体只解一层：&amp;lt; 是字面的 &lt;，不是 <', () => {
    assert.equal(htmlToMarkdown('<p>a &amp;lt;b&amp;gt; &amp;amp; c</p>'), 'a &lt;b&gt; &amp; c')
    assert.equal(htmlToMarkdown('<p>&lt;tag&gt; &amp; &#60;</p>'), '<tag> & <')
  })

  it('fileSlug 留下中文', () => {
    assert.equal(fileSlug('第一篇 笔记', 0), '第一篇-笔记')
    assert.equal(fileSlug('   ', 2), 'note-3')
  })
})

describe('wordpress wxr', () => {
  it('跳过附件，正文从 content:encoded 转成 markdown', () => {
    const notes = parseWordpressXml(`<?xml version="1.0"?>
<rss xmlns:content="http://purl.org/rss/1.0/modules/content/"
     xmlns:wp="http://wordpress.org/export/1.2/">
<channel>
  <item>
    <title>Hello</title>
    <content:encoded><![CDATA[<p>Hi <strong>WP</strong></p>]]></content:encoded>
    <category><![CDATA[tech]]></category>
    <wp:post_type>post</wp:post_type>
    <wp:status>publish</wp:status>
    <wp:post_date_gmt>2023-11-14 22:13:20</wp:post_date_gmt>
  </item>
  <item>
    <title>pic</title>
    <wp:post_type>attachment</wp:post_type>
    <wp:status>inherit</wp:status>
  </item>
</channel>
</rss>`)
    assert.equal(notes.length, 1)
    assert.equal(notes[0]!.title, 'Hello')
    assert.equal(notes[0]!.status, 'published')
    assert.equal(notes[0]!.topic, 'tech')
    assert.match(notes[0]!.content_md, /\*\*WP\*\*/)
    assert.equal(notes[0]!.published_at, 1_700_000_000)
  })

  it('往返保住标题、主题和状态', () => {
    const xml = serializeWordpressXml([sample], 'Phantasi')
    const notes = parseWordpressXml(xml)
    assert.equal(notes[0]!.title, sample.title)
    assert.equal(notes[0]!.topic, sample.topic)
    assert.equal(notes[0]!.status, 'published')
    assert.match(notes[0]!.content_md, /世界/)
  })
})

describe('halo json', () => {
  it('认 spec + content.raw，也认 notes 数组', () => {
    const native = parseHaloJson(
      JSON.stringify({
        posts: [
          {
            spec: {
              title: 'Halo 文',
              publish: true,
              publishTime: '2023-11-14T22:13:20.000Z',
              tags: ['日常'],
            },
            content: { raw: '# 标题\n\n正文', rawType: 'markdown' },
          },
        ],
      }),
    )
    assert.equal(native[0]!.title, 'Halo 文')
    assert.equal(native[0]!.status, 'published')
    assert.equal(native[0]!.topic, '日常')
    assert.equal(native[0]!.content_md, '# 标题\n\n正文')

    const notes = parseHaloJson(
      JSON.stringify({
        notes: [{ title: '草稿', content_md: 'x', status: 'draft' }],
      }),
    )
    assert.equal(notes[0]!.status, 'draft')
  })

  it('往返保住 markdown 正文', () => {
    const parsed = parseHaloJson(serializeHaloJson([sample]))
    assert.equal(parsed[0]!.title, sample.title)
    assert.equal(parsed[0]!.content_md, sample.content_md)
    assert.equal(parsed[0]!.topic, sample.topic)
    assert.equal(parsed[0]!.status, 'published')
  })
})

describe('typecho xml', () => {
  it('认原生备份，也把 WXR 交给 wordpress 解析', () => {
    const notes = parseTypechoXml(`<?xml version="1.0"?>
<typecho version="1.2">
<contents>
  <item>
    <title>茶</title>
    <created>1700000000</created>
    <text>喝一杯</text>
    <type>post</type>
    <status>publish</status>
    <categories>生活</categories>
  </item>
  <item>
    <title>附件</title>
    <type>attachment</type>
    <text>x</text>
    <status>publish</status>
  </item>
</contents>
</typecho>`)
    assert.equal(notes.length, 1)
    assert.equal(notes[0]!.title, '茶')
    assert.equal(notes[0]!.content_md, '喝一杯')
    assert.equal(notes[0]!.topic, '生活')
    assert.equal(notes[0]!.status, 'published')

    const wxr = serializeWordpressXml([sample], 'Phantasi')
    const fromWxr = parseTypechoXml(wxr)
    assert.equal(fromWxr[0]!.title, sample.title)
  })

  it('往返保住标题和正文', () => {
    const parsed = parseTypechoXml(serializeTypechoXml([sample]))
    assert.equal(parsed[0]!.title, sample.title)
    assert.equal(parsed[0]!.content_md, sample.content_md)
    assert.equal(parsed[0]!.topic, sample.topic)
    assert.equal(parsed[0]!.status, 'published')
    assert.equal(parsed[0]!.published_at, sample.published_at)
  })
})

describe('markdown front matter', () => {
  it('读 YAML 头，没有头时用文件名或一级标题', () => {
    const withMeta = parseMarkdownFile(
      'x.md',
      '---\ntitle: 有头\ntopic: 日常\nstatus: draft\ndate: 2023-11-14T22:13:20.000Z\n---\n\n正文\n',
    )
    assert.equal(withMeta.title, '有头')
    assert.equal(withMeta.topic, '日常')
    assert.equal(withMeta.status, 'draft')
    assert.equal(withMeta.content_md, '正文')
    assert.equal(withMeta.published_at, 1_700_000_000)

    const heading = parseMarkdownFile('ignored.md', '# 标题\n\n一段')
    assert.equal(heading.title, '标题')
    assert.equal(heading.content_md, '一段')
    assert.equal(heading.status, 'published')

    const bare = parseMarkdownFile('plain-name.md', '只有正文')
    assert.equal(bare.title, 'plain-name')
    assert.equal(bare.content_md, '只有正文')
  })

  it('往返保住头信息', () => {
    const text = serializeMarkdownFile(sample)
    const parsed = parseMarkdownFile('out.md', text)
    assert.equal(parsed.title, sample.title)
    assert.equal(parsed.content_md, sample.content_md)
    assert.equal(parsed.topic, sample.topic)
    assert.equal(parsed.status, 'published')
  })
})

describe('parseTransferFile', () => {
  it('按种类读 xml / json / md', async () => {
    const wp = await parseTransferFile(
      'wordpress',
      new File([serializeWordpressXml([sample], 'Phantasi')], 'a.xml'),
    )
    assert.equal(wp[0]!.title, sample.title)

    const halo = await parseTransferFile(
      'halo',
      new File([serializeHaloJson([sample])], 'a.json'),
    )
    assert.equal(halo[0]!.content_md, sample.content_md)

    const typecho = await parseTransferFile(
      'typecho',
      new File([serializeTypechoXml([sample])], 'a.xml'),
    )
    assert.equal(typecho[0]!.title, sample.title)

    const md = await parseTransferFile(
      'markdown',
      new File([serializeMarkdownFile(sample)], 'a.md'),
    )
    assert.equal(md[0]!.title, sample.title)
  })
})
