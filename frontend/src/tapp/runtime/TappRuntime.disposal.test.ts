import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import ts from 'typescript'

const source = ts.createSourceFile('runtime.ts', readFileSync(new URL('./TappRuntime.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true)
const body = source.statements.filter(node => !ts.isImportDeclaration(node)).map(node => node.getText(source).replace(/^export /, '')).join('\n')
function runtimeWith(dependencies: Record<string, unknown>): typeof import('./TappRuntime').TappRuntime {
  const deps = { tappKernel: { resources: { clearCache() {} } }, ...dependencies }
  return compileFunction(`${ts.transpile(body, { target: ts.ScriptTarget.ESNext })}; return TappRuntime`, Object.keys(deps))(...Object.values(deps))
}
const installed = { id: 'owner-app', manifest: { id: 'owner-app', permissions: [] }, user_role: 'admin', is_admin_tapp: true, status: 'installed', granted_permissions: [] }

test('a response arriving after reset cannot repopulate the disposed runtime', async () => {
  const held = Promise.withResolvers<unknown[]>()
  let signal: AbortSignal | undefined
  const Runtime = runtimeWith({ listTappDetails: (_scope: unknown, next: AbortSignal) => { signal = next; return held.promise }, getAllWidgets: async () => [] })
  const old = Runtime.getInstance()
  const pending = old.waitForSync()
  Runtime.reset()
  assert.equal(signal?.aborted, true)
  held.resolve([installed])
  await assert.rejects(pending, { name: 'AbortError' })
  assert.deepEqual(old.getAllTapps(), [])
  assert.deepEqual(old.getRegisteredWidgets(), [])
  await assert.rejects(old.syncFromBackend(true), { name: 'AbortError' })
})

test('reset discards an in-flight lifecycle result and prevents queued network writes', async () => {
  const held = Promise.withResolvers<void>()
  let starts = 0
  let stops = 0
  const Runtime = runtimeWith({
    listTappDetails: async () => [installed], getAllWidgets: async () => [],
    startTapp: async () => { starts++; await held.promise },
    stopTapp: async () => { stops++ },
  })
  const old = Runtime.getInstance()
  await old.waitForSync()
  const starting = old.startTapp(installed.id)
  const stopping = old.stopTapp(installed.id)
  const rejectedStart = assert.rejects(starting, { name: 'AbortError' })
  const rejectedStop = assert.rejects(stopping, { name: 'AbortError' })
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(starts, 1)
  Runtime.reset()
  held.resolve()
  await Promise.all([rejectedStart, rejectedStop])
  assert.equal(stops, 0)
  assert.equal(old.isRunning(installed.id), false)
  assert.deepEqual(old.getAllTapps(), [])
})
