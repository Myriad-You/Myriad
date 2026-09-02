import assert from 'node:assert/strict'
import test from 'node:test'
import { agentContextRoute, resolveAgentContext } from './agentContext'

test('认得出常见路由，也认得出首页', () => {
  assert.equal(agentContextRoute('/'), 'home')
  assert.equal(agentContextRoute(''), 'home')
  assert.equal(agentContextRoute('/brew'), 'brew')
  assert.equal(agentContextRoute('/brew/item/42'), 'brew')
  assert.equal(agentContextRoute('/config'), 'config')
  assert.equal(agentContextRoute('/tapp/run/abc'), 'tapp')
})

test('只认整段前缀，不把名字相近的路由算进来', () => {
  assert.equal(agentContextRoute('/librarything'), 'other')
  assert.equal(agentContextRoute('/library'), 'library')
  assert.equal(agentContextRoute('/library/canvas'), 'library')
})

test('认不出来的路由老实说不知道，不瞎猜', () => {
  assert.equal(agentContextRoute('/whatever'), 'other')
})

test('有正文时报标题，没正文时只报在哪一页', () => {
  assert.deepEqual(
    resolveAgentContext({
      pathname: '/brew/item/42',
      pageTitle: '  一篇文章  ',
      hasPageContent: true,
    }),
    { kind: 'content', route: 'brew', title: '一篇文章' },
  )

  assert.deepEqual(
    resolveAgentContext({ pathname: '/library', hasPageContent: false }),
    { kind: 'route', route: 'library' },
  )
})

test('有正文但没标题仍然算看得到内容 —— 能总结的是正文不是标题', () => {
  assert.deepEqual(
    resolveAgentContext({
      pathname: '/brew/item/42',
      pageTitle: '   ',
      hasPageContent: true,
    }),
    { kind: 'content', route: 'brew' },
  )
})

test('标题在但没正文时不谎称看得到内容', () => {
  assert.deepEqual(
    resolveAgentContext({
      pathname: '/reports',
      pageTitle: '数据报告',
      hasPageContent: false,
    }),
    { kind: 'route', route: 'reports' },
  )
})

test('选中的那段压过页面正文 —— 指着的东西比在哪儿具体', () => {
  assert.deepEqual(
    resolveAgentContext({
      pathname: '/brew/item/42',
      pageTitle: '一篇文章',
      hasPageContent: true,
      selection: '这一段话',
    }),
    { kind: 'selection', route: 'brew', selection: '这一段话' },
  )
})

test('关掉读页之后，页面正文这一路当不存在', () => {
  assert.deepEqual(
    resolveAgentContext({
      pathname: '/brew/item/42',
      pageTitle: '一篇文章',
      hasPageContent: true,
      contextConsent: false,
    }),
    { kind: 'route', route: 'brew' },
  )
})

test('关掉读页不影响用户自己划出来的那段', () => {
  const context = resolveAgentContext({
    pathname: '/brew/item/42',
    pageTitle: '一篇文章',
    hasPageContent: true,
    selection: '这一段话',
    contextConsent: false,
  })
  assert.equal(context.kind, 'selection')
  assert.equal(context.selection, '这一段话')
})

test('没说开关就当开着 —— 站点助手读当前页是本职', () => {
  assert.equal(
    resolveAgentContext({
      pathname: '/brew/item/42',
      pageTitle: '一篇文章',
      hasPageContent: true,
    }).kind,
    'content',
  )
})
