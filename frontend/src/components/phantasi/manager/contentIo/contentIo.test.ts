import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ApiError } from '../../../../services/api'
import { parseTransferFile, publishImported } from './contentIo.ts'
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

describe('旧地址和 slug', () => {
  it('WordPress：<link> 的路径当旧地址，wp:post_name 解码后能用才当 slug', () => {
    const notes = parseWordpressXml(`<?xml version="1.0"?>
<rss xmlns:content="http://purl.org/rss/1.0/modules/content/"
     xmlns:wp="http://wordpress.org/export/1.2/">
<channel>
  <item>
    <title>Hello</title>
    <link>https://old.example/2024/05/hello-world/</link>
    <content:encoded><![CDATA[<p>Hi</p>]]></content:encoded>
    <wp:post_name><![CDATA[hello-world]]></wp:post_name>
    <wp:post_type>post</wp:post_type>
    <wp:status>publish</wp:status>
  </item>
  <item>
    <title>你好</title>
    <link>https://old.example/category/%e4%bd%a0%e5%a5%bd/</link>
    <content:encoded><![CDATA[<p>中文</p>]]></content:encoded>
    <wp:post_name><![CDATA[%e4%bd%a0%e5%a5%bd]]></wp:post_name>
    <wp:post_type>post</wp:post_type>
    <wp:status>publish</wp:status>
  </item>
  <item>
    <title>草稿</title>
    <link>https://old.example/?p=12</link>
    <content:encoded><![CDATA[<p>x</p>]]></content:encoded>
    <wp:post_name><![CDATA[]]></wp:post_name>
    <wp:post_type>post</wp:post_type>
    <wp:status>draft</wp:status>
  </item>
</channel>
</rss>`)
    assert.equal(notes[0]!.slug, 'hello-world')
    assert.deepEqual(notes[0]!.old_paths, ['/2024/05/hello-world'])
    assert.equal(notes[0]!.topic, null)
    assert.equal(notes[1]!.slug, undefined)
    assert.deepEqual(notes[1]!.old_paths, ['/category/你好'])
    assert.equal(notes[2]!.slug, undefined)
    assert.equal(notes[2]!.old_paths, undefined)
  })

  const moved = { ...sample, slug: 'first-post', old_paths: ['/2024/05/first', '/archives/12'] }

  it('WordPress、Halo、Markdown 往返带着 slug 和旧地址，Typecho 带着 slug', () => {
    for (const parsed of [
      parseWordpressXml(serializeWordpressXml([moved], 'Phantasi'))[0]!,
      parseHaloJson(serializeHaloJson([moved]))[0]!,
      parseMarkdownFile('out.md', serializeMarkdownFile(moved)),
    ]) {
      assert.equal(parsed.slug, 'first-post')
      assert.deepEqual(parsed.old_paths, moved.old_paths)
    }
    assert.equal(parseTypechoXml(serializeTypechoXml([moved]))[0]!.slug, 'first-post')
    // 没有地址的照旧导出，不多出空字段。
    assert.doesNotMatch(serializeWordpressXml([sample], 'P'), /post_name|postmeta/)
    assert.doesNotMatch(serializeMarkdownFile(sample), /aliases|slug/)
  })

  it('Halo 2 原生备份认 spec.slug 和 status.permalink', () => {
    const [note] = parseHaloJson(JSON.stringify({
      posts: [{
        spec: { title: 'Halo', slug: 'halo-post', publish: true },
        status: { phase: 'PUBLISHED', permalink: '/archives/halo-post' },
        content: { raw: 'x' },
      }],
    }))
    assert.equal(note!.slug, 'halo-post')
    assert.deepEqual(note!.old_paths, ['/archives/halo-post'])
  })

  it('Typecho 备份里的 slug 和地址', () => {
    const [note] = parseTypechoXml(`<typecho><contents><item>
    <title>茶</title><text>x</text><type>post</type><status>publish</status>
    <slug>tea</slug><permalink>https://old.example/archives/tea.html</permalink>
  </item></contents></typecho>`)
    assert.equal(note!.slug, 'tea')
    assert.deepEqual(note!.old_paths, ['/archives/tea.html'])
  })

  it('Markdown 头里的 permalink、url、aliases、slug（YAML 和 TOML）', () => {
    const yaml = parseMarkdownFile('a.md', [
      '---',
      'title: Hexo',
      'slug: Hello-World',
      'permalink: 2024/05/hello/',
      'url: /old/url/',
      'aliases:',
      '  - /a/',
      "  - '/b/'",
      '---',
      '正文',
    ].join('\n'))
    assert.equal(yaml.slug, 'hello-world')
    assert.deepEqual(yaml.old_paths, ['/2024/05/hello', '/old/url', '/a', '/b'])
    const toml = parseMarkdownFile('b.md', [
      '+++',
      'title = "Hugo"',
      'slug = "hugo-post"',
      'aliases = ["/posts/old/", "/api/nope/"]',
      '+++',
      '正文',
    ].join('\n'))
    assert.equal(toml.title, 'Hugo')
    assert.equal(toml.slug, 'hugo-post')
    // 站内地址丢掉，不让一条坏地址拖垮整篇导入。
    assert.deepEqual(toml.old_paths, ['/posts/old'])
    const inline = parseMarkdownFile('c.md', '---\ntitle: x\naliases: [/x/, "/y/"]\n---\n正文')
    assert.deepEqual(inline.old_paths, ['/x', '/y'])
  })

  it('导入发布：slug 被占用退回自动分配，旧地址被占用就不带旧地址', async () => {
    const calls: Array<Record<string, unknown>> = []
    const dropped: unknown[] = []
    const publish = (async (_id: number, req: Record<string, unknown> = {}) => {
      calls.push(req)
      if (req.slug) throw new ApiError('That slug is already used', 409)
      if (req.old_paths) throw new ApiError('The old address "/a" already points to another note', 409)
      return { id: 1, link: '/blog/posts/x/', doc: {} }
    }) as unknown as Parameters<typeof publishImported>[0]
    await publishImported(publish, 7, 3, { title: 't', content_md: 'b', topic: null, published_at: null }, {
      ...sample, slug: 'taken', old_paths: ['/a'],
    }, (err) => dropped.push(err))
    assert.equal(calls.length, 3)
    assert.equal(calls[0]!.slug, 'taken')
    assert.equal(calls[1]!.slug, undefined)
    assert.deepEqual(calls[1]!.old_paths, ['/a'])
    assert.equal(calls[2]!.old_paths, undefined)
    assert.equal(calls[2]!.revision, 3)
    assert.equal(dropped.length, 1)

    const other = (async () => {
      throw new ApiError('Note draft was updated elsewhere', 409)
    }) as unknown as Parameters<typeof publishImported>[0]
    await assert.rejects(publishImported(other, 7, 3, { title: 't', content_md: 'b', topic: null, published_at: null }, sample, () => {}))
  })
})
