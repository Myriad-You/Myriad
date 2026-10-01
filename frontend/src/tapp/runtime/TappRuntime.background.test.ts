import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import ts from 'typescript'

const source = ts.createSourceFile('runtime.ts', readFileSync(new URL('./TappRuntime.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true)
const body = source.statements.filter(node => !ts.isImportDeclaration(node)).map(node => node.getText(source).replace(/^export /, '')).join('\n')
function runtimeFor(details: unknown[]): typeof import('./TappRuntime').TappRuntime {
  const deps = { getResourceLoader: () => ({ clearCache() {} }), listTappDetails: async () => details, getAllWidgets: async () => [] }
  return compileFunction(`${ts.transpile(body, { target: ts.ScriptTarget.ESNext })}; return TappRuntime`, Object.keys(deps))(...Object.values(deps))
}
// 站主公开且在运行的 Tapp：每个访客都会拉起它的后台 core。
function publicTapp(role: string, backgroundRequirements: string[]) {
  return {
    id: 'public-app', manifest: { id: 'public-app', permissions: [], backgroundRequirements },
    user_role: role, is_admin_tapp: true, status: 'running', granted_permissions: [],
  }
}

test('a guest does not keep a Tapp resident only for notifications it can never receive', async () => {
  const Runtime = runtimeFor([publicTapp('guest', ['notification'])])
  const runtime = Runtime.getInstance()
  await runtime.waitForSync()
  assert.equal(runtime.isRunning('public-app'), true)
  assert.deepEqual(runtime.getBackgroundRequirements('public-app'), [])
  assert.deepEqual(runtime.getBackgroundTapps(), [])
  runtime.registerBackgroundRequirement('public-app', 'notification')
  assert.deepEqual(runtime.getBackgroundTapps(), [])
  Runtime.reset()
})

test('a guest keeps the background requirements that still work without an account', async () => {
  const Runtime = runtimeFor([publicTapp('guest', ['notification', 'media'])])
  const runtime = Runtime.getInstance()
  await runtime.waitForSync()
  assert.deepEqual(runtime.getBackgroundRequirements('public-app'), ['media'])
  assert.deepEqual(runtime.getBackgroundTapps().map(tapp => tapp.id), ['public-app'])
  Runtime.reset()
})

test('a signed-in user keeps a notification-only Tapp resident', async () => {
  const Runtime = runtimeFor([publicTapp('user', ['notification'])])
  const runtime = Runtime.getInstance()
  await runtime.waitForSync()
  assert.deepEqual(runtime.getBackgroundRequirements('public-app'), ['notification'])
  assert.deepEqual(runtime.getBackgroundTapps().map(tapp => tapp.id), ['public-app'])
  Runtime.reset()
})
