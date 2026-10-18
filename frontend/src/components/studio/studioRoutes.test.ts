import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { pageIdFromPath } from '../../hooks/animation/pageId.ts'
import { resolvePageRouteAnimation } from '../../tapp/routing/tappRouteMeta.ts'
import {
  parseStudioPath,
  studioAccess,
  studioMediaOrigin,
  studioNotePath,
  studioSectionForMediaOrigin,
  studioSectionPath,
} from './studioRoutes.ts'

describe('parseStudioPath', () => {
  it('创作侧各有地址，订阅面板不在其中', () => {
    assert.deepEqual(parseStudioPath('/studio'), { kind: 'section', section: 'home' })
    assert.deepEqual(parseStudioPath('/studio/'), { kind: 'section', section: 'home' })
    assert.deepEqual(parseStudioPath('/studio/notes'), { kind: 'section', section: 'notes' })
    assert.deepEqual(parseStudioPath('/studio/notes/new'), { kind: 'editor', docId: 'new' })
    assert.deepEqual(parseStudioPath('/studio/notes/12'), { kind: 'editor', docId: 12 })
    assert.equal(parseStudioPath('/studio/notes/0'), null)
    assert.equal(parseStudioPath('/studio/notes/new/extra'), null)
    assert.deepEqual(parseStudioPath('/studio/comments'), { kind: 'section', section: 'comments' })
    assert.deepEqual(parseStudioPath('/studio/media'), { kind: 'section', section: 'media' })
    assert.deepEqual(parseStudioPath('/studio/media/uploads'), { kind: 'section', section: 'mediaUploads' })
    assert.deepEqual(parseStudioPath('/studio/media/generated'), { kind: 'section', section: 'mediaGenerated' })
    assert.deepEqual(parseStudioPath('/studio/media/kept'), { kind: 'section', section: 'mediaKept' })
    assert.deepEqual(parseStudioPath('/studio/media/channel'), { kind: 'section', section: 'mediaChannel' })
    assert.deepEqual(parseStudioPath('/studio/media/imported'), { kind: 'section', section: 'mediaImported' })
    assert.equal(parseStudioPath('/studio/media/unknown'), null)
    assert.deepEqual(parseStudioPath('/studio/reviews'), { kind: 'section', section: 'reviews' })
    assert.deepEqual(parseStudioPath('/studio/categories'), { kind: 'section', section: 'categories' })
    assert.deepEqual(parseStudioPath('/studio/import'), { kind: 'section', section: 'import' })
    assert.deepEqual(parseStudioPath('/studio/blog'), { kind: 'section', section: 'blog' })
    assert.equal(parseStudioPath('/studio/feeds'), null)
    assert.equal(parseStudioPath('/journal/workbench/notes'), null)
    assert.equal(studioSectionPath('home'), '/studio')
    assert.equal(studioSectionPath('import'), '/studio/import')
    assert.equal(studioSectionPath('mediaKept'), '/studio/media/kept')
    assert.equal(studioMediaOrigin('media'), 'all')
    assert.equal(studioMediaOrigin('mediaUploads'), 'upload')
    assert.equal(studioMediaOrigin('mediaGenerated'), 'generated')
    assert.equal(studioMediaOrigin('mediaKept'), 'kept')
    assert.equal(studioMediaOrigin('mediaChannel'), 'channel')
    assert.equal(studioMediaOrigin('mediaImported'), 'import')
    assert.equal(studioMediaOrigin('notes'), null)
    assert.equal(studioSectionForMediaOrigin('kept'), 'mediaKept')
    assert.equal(studioSectionForMediaOrigin('generated'), 'mediaGenerated')
    assert.equal(studioNotePath('new'), '/studio/notes/new')
    assert.equal(studioNotePath(4), '/studio/notes/4')
  })

  it('非管理员去登录或回首页', () => {
    assert.equal(studioAccess({ hasChecked: false, isAuthenticated: false, isAdmin: false }), 'pending')
    assert.equal(studioAccess({ hasChecked: true, isAuthenticated: false, isAdmin: false }), 'login')
    assert.equal(studioAccess({ hasChecked: true, isAuthenticated: true, isAdmin: false }), 'home')
    assert.equal(studioAccess({ hasChecked: true, isAuthenticated: true, isAdmin: true }), 'allow')
  })

  it('子路径共用一页动画', () => {
    assert.equal(pageIdFromPath('/studio'), 'studio')
    assert.equal(pageIdFromPath('/studio/notes/new'), 'studio')
    assert.equal(resolvePageRouteAnimation('/studio').key, '/studio')
    assert.equal(resolvePageRouteAnimation('/studio/blog').key, '/studio')
  })
})
