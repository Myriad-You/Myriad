import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  blogSettingsTabForArg,
  editorTarget,
  legacyStudioRedirect,
  mediaOriginForArg,
  parseStudioToolPath,
} from './studioRoutes.ts'

describe('studio tool routes', () => {
  it('工具窗口各有地址，订阅面板不在其中', () => {
    assert.equal(parseStudioToolPath('/embed/studio/tool/comments'), 'comments')
    assert.equal(parseStudioToolPath('/embed/studio/tool/editor/'), 'editor')
    assert.equal(parseStudioToolPath('/embed/studio/tool/feeds'), null)
    assert.equal(parseStudioToolPath('/embed/studio/overlay'), null)
    assert.equal(parseStudioToolPath('/studio/notes'), null)
  })

  it('编辑器参数只认正整数 id，其余当新笔记', () => {
    assert.equal(editorTarget('12'), 12)
    assert.equal(editorTarget('new'), 'new')
    assert.equal(editorTarget('0'), 'new')
    assert.equal(editorTarget(null), 'new')
  })

  it('媒体参数对应来源', () => {
    assert.equal(mediaOriginForArg('kept'), 'kept')
    assert.equal(mediaOriginForArg('imported'), 'import')
    assert.equal(mediaOriginForArg('uploads'), 'upload')
    assert.equal(mediaOriginForArg(null), 'all')
    assert.equal(blogSettingsTabForArg('themes'), 'themes')
    assert.equal(blogSettingsTabForArg('build'), 'build')
    assert.equal(blogSettingsTabForArg('nope'), 'site')
    assert.equal(blogSettingsTabForArg(null), 'site')
  })

  it('旧地址跳去博客页并打开对应工具', () => {
    assert.equal(legacyStudioRedirect('/studio'), '/blog/?studio=overview')
    assert.equal(legacyStudioRedirect('/studio/'), '/blog/?studio=overview')
    assert.equal(legacyStudioRedirect('/studio/notes/12'), '/blog/?studio=editor-12')
    assert.equal(legacyStudioRedirect('/studio/media/kept/'), '/blog/?studio=media-kept')
    assert.equal(legacyStudioRedirect('/studio/blog'), '/blog/?studio=settings')
  })
})
