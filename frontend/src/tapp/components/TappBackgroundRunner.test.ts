import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import React from 'react'
import ts from 'typescript'

// Execute the real orchestration with the network and React effects under test control.
const source = ts.createSourceFile('runner.tsx', readFileSync(new URL('./TappBackgroundRunner.tsx', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
const body = source.statements.filter(node => !ts.isImportDeclaration(node) && !ts.isExportAssignment(node)).map(node => node.getText(source).replace(/^export /, '')).join('\n')
const script = ts.transpile(body, { target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.React })

for (const failed of [0, 4]) {
test(`background runner admits four successful instances serially after ${failed} failures`, async () => {
  const effects: Array<() => (() => void) | undefined> = []
  const updates: unknown[] = []
  const events = new Map<string, () => void>()
  const calls: string[] = []
  const published: Array<Array<{ id: string }>> = []
  let stopResident: ((tappId: string) => void) | undefined
  const first = Promise.withResolvers<void>()
  const tapps = Array.from({ length: 20 }, (_, i) => ({ id: String(i), manifest: { version: '1', name: String(i) } }))
  const dependencies = {
    window: { addEventListener() {}, removeEventListener() {} },
    React, useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, (value: unknown) => updates.push(value)],
    useCallback: (callback: unknown) => callback, useEffect: (effect: () => (() => void) | undefined) => effects.push(effect),
    getTappRuntime: () => ({ waitForSync: async () => {}, getBackgroundTapps: () => tapps, getBackgroundRequirements: () => ['sync'], on: (event: string, callback: () => void) => { events.set(event, callback); return () => {} } }),
    publishBackgroundResidents: (next: Array<{ id: string }>) => published.push(next),
    clearBackgroundResidents: () => {},
    registerBackgroundResidentStopHandler: (handler: (tappId: string) => void) => { stopResident = handler; return () => { stopResident = undefined } },
    tappKernel: { resources: { loadCoreResources: async (tapp: { id: string }) => { calls.push(tapp.id); if (tapp.id === '0') await first.promise; if (Number(tapp.id) < failed) throw new Error('fixture package unavailable'); return { modules: {} } } } },
    lazy: () => () => null, Suspense: React.Suspense,
  }
  const runner = compileFunction(`${script}; return TappBackgroundRunner;`, Object.keys(dependencies))(...Object.values(dependencies))
  runner()
  const cleanups = effects.map(effect => effect())
  await new Promise(resolve => setImmediate(resolve))
  assert.deepEqual(calls, ['0'])
  first.resolve()
  await new Promise(resolve => setImmediate(resolve))
  assert.deepEqual(calls, Array.from({ length: failed + 4 }, (_, i) => String(i)))
  assert.deepEqual((updates[0] as Array<{ id: string }>).map(tapp => tapp.id), Array.from({ length: 4 }, (_, i) => String(i + failed)))
  assert.deepEqual(published.at(-1)?.map(tapp => tapp.id), Array.from({ length: 4 }, (_, i) => String(i + failed)))
  const installed = updates[0] as Array<{ id: string }>
  const nextUpdate = updates.length
  events.get('sync:complete')!()
  assert.deepEqual((updates[nextUpdate] as typeof installed).map(tapp => tapp.id), installed.map(tapp => tapp.id), 'sync must retain admitted apps beyond failed candidates')
  stopResident!(installed[0].id)
  assert.deepEqual(published.at(-1)?.map(tapp => tapp.id), installed.slice(1).map(tapp => tapp.id))
  cleanups.forEach(cleanup => cleanup?.())
})
}

test('headless sandbox does not read t.tapp (background runner is outside I18nNamespace)', () => {
  const sandbox = readFileSync(new URL('../runtime/TappPageSandbox.tsx', import.meta.url), 'utf8')
  assert.match(sandbox, /headless \? undefined : t\.tapp\.cannotLoadApp/)
})
