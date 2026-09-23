import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  displayImageUrl,
  prepareNoteReaderHtml,
  withDisplayImages,
} from './noteImageUrl'

const gold = join(
  dirname(fileURLToPath(import.meta.url)),
  '../../../../../crates/myriad-phantasi-notes/testdata',
)
const golden = (name: string) => readFileSync(join(gold, name), 'utf8')

const api = 'http://localhost:3000'

describe('displayImageUrl', () => {
  it('本站媒体：不管存的是哪个域名，都改成当前 API origin 下的 /api 别名', () => {
    assert.equal(
      displayImageUrl('https://my.site/media/federation/1/a.jpg', api),
      'http://localhost:3000/api/media/federation/1/a.jpg',
    )
    assert.equal(displayImageUrl('/media/federation/1/a.jpg', api), 'http://localhost:3000/api/media/federation/1/a.jpg')
    assert.equal(
      displayImageUrl('/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.jpg', api),
      'http://localhost:3000/api/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.jpg',
    )
    assert.equal(displayImageUrl('/api/x.png?v=2', api), 'http://localhost:3000/api/x.png?v=2')
  })

  it('外站图原样（或按热链名单代理），data/blob 不动', () => {
    assert.equal(displayImageUrl('https://x.y/p.png', api), 'https://x.y/p.png')
    assert.equal(displayImageUrl('data:image/png;base64,AAA', api), 'data:image/png;base64,AAA')
    assert.equal(displayImageUrl('not a url', api), 'not a url')
  })
})

describe('withDisplayImages', () => {
  it('只改 src，alt 和别的属性不动，& 转义来回一致', () => {
    const html = '<p><img alt="封面" src="https://my.site/media/federation/1/a.jpg?x=1&amp;y=2"></p>'
    assert.equal(
      withDisplayImages(html, api),
      '<p><img alt="封面" src="http://localhost:3000/api/media/federation/1/a.jpg?x=1&amp;y=2"></p>',
    )
  })
})

describe('prepareNoteReaderHtml', () => {
  it('空正文用占位；小组件属性和配置原样留下', () => {
    assert.equal(prepareNoteReaderHtml('', '<p>EMPTY</p>', api), '<p>EMPTY</p>')
    const widget =
      '<div class="note-widget" data-widget="weather" data-size="2x2" data-config="%7B%22city%22%3A%22Tokyo%22%7D">weather</div>'
    assert.equal(
      prepareNoteReaderHtml(widget, '<p>EMPTY</p>', api),
      '<div class="note-widget not-prose" data-widget="weather" data-size="2x2" data-config="%7B%22city%22%3A%22Tokyo%22%7D"></div>',
    )
    assert.equal(
      prepareNoteReaderHtml(
        '<div class="note-widget not-prose" data-widget="quote" data-size="2x2">quote</div>',
        '<p>EMPTY</p>',
        api,
      ),
      '<div class="note-widget not-prose" data-widget="quote" data-size="2x2"></div>',
    )
  })

  it('预览去掉 data-md-* 后和发布 HTML 同一份准备；无 iframe / 密钥', () => {
    // 与 `preview_keeps_link_defs_before_footnotes_when_stamped` 同一份渲出。
    const published = golden('link-defs.published.html')
    const preview = golden('link-defs.preview.html')
    assert.equal(stripPreviewRanges(preview), published)
    assert.equal(
      prepareNoteReaderHtml(published, '<p>EMPTY</p>', api),
      prepareNoteReaderHtml(stripPreviewRanges(preview), '<p>EMPTY</p>', api),
    )
    const prepared = prepareNoteReaderHtml(published, '<p>EMPTY</p>', api)
    assert.match(prepared, /data-widget="weather"/)
    assert.match(prepared, /data-widget="friend-links"/)
    assert.match(prepared, /class="link-definition"/)
    assert.doesNotMatch(prepared, /iframe|HOST_SECRET|>weather<|>friend-links</)
    const link = prepared.indexOf('class="link-definition"')
    const foot = prepared.indexOf('class="footnote-definition"')
    assert.ok(link >= 0 && foot > link)
  })

  it('友链 / 报告卡 / Tapp 的预览去掉区间后等于发布 HTML；无 iframe / 密钥', () => {
    // 与 `preview_friend_report_tapp_match_publish_and_drop_secret_iframe` 同一份渲出。
    const published = golden('friend-report-tapp.published.html')
    const preview = golden('friend-report-tapp.preview.html')
    assert.equal(stripPreviewRanges(preview), published)
    assert.equal(
      prepareNoteReaderHtml(published, '<p>EMPTY</p>', api),
      prepareNoteReaderHtml(stripPreviewRanges(preview), '<p>EMPTY</p>', api),
    )
    const prepared = prepareNoteReaderHtml(published, '<p>EMPTY</p>', api)
    assert.match(prepared, /data-widget="friend-links"/)
    assert.match(prepared, /data-widget="report-bilibili"/)
    assert.match(prepared, /data-widget="tapp-shortcut"/)
    assert.doesNotMatch(prepared, /iframe|HOST_SECRET|>friend-links<|>report-bilibili<|>tapp-shortcut</)
  })

  it('分栏里的小组件：预览去掉区间后和发布 HTML 同一份准备', () => {
    const published = golden('columns-widget.published.html')
    const preview = golden('columns-widget.preview.html')
    assert.equal(stripPreviewRanges(preview), published)
    assert.equal(
      prepareNoteReaderHtml(published, '<p>EMPTY</p>', api),
      prepareNoteReaderHtml(stripPreviewRanges(preview), '<p>EMPTY</p>', api),
    )
    const prepared = prepareNoteReaderHtml(published, '<p>EMPTY</p>', api)
    assert.match(prepared, /class="note-columns"/)
    assert.match(prepared, /data-widget="quote"/)
    assert.match(prepared, /class="footnote-definition"/)
    assert.doesNotMatch(prepared, />quote</)
  })
})

function stripPreviewRanges(html: string): string {
  let out = ''
  let rest = html
  while (true) {
    const at = rest.indexOf(' data-md-')
    if (at < 0) {
      out += rest
      return out
    }
    out += rest.slice(0, at)
    const after = rest.slice(at + 1)
    const first = after.indexOf('"')
    const second = first < 0 ? -1 : after.indexOf('"', first + 1)
    if (second < 0) {
      out += rest.slice(at)
      return out
    }
    rest = after.slice(second + 1)
  }
}
