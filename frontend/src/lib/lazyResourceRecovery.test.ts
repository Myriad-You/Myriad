import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import ts from 'typescript'

// Keep the real loader state machines; replace only the external import boundary.
function loadSource(url: URL, loader: () => Promise<unknown>, declarations?: string[]) {
  const source = ts.createSourceFile('loader.ts', readFileSync(url, 'utf8'), ts.ScriptTarget.Latest, true)
  const statements = source.statements.filter(node => {
    if (ts.isImportDeclaration(node)) return false
    if (!declarations) return true
    if (ts.isFunctionDeclaration(node)) return !!node.name && declarations.includes(node.name.text)
    return ts.isVariableStatement(node) && node.declarationList.declarations.some(declaration => ts.isIdentifier(declaration.name) && declarations.includes(declaration.name.text))
  })
  const input = ts.factory.updateSourceFile(source, statements)
  const transformed = ts.transform(input, [context => root => ts.visitNode(root, function visit(node): ts.VisitResult<ts.Node> {
    if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
      return ts.factory.createCallExpression(ts.factory.createIdentifier('TestImport'), undefined, [])
    }
    return ts.visitEachChild(node, visit, context)
  }) as ts.SourceFile])
  try {
    const code = ts.transpileModule(ts.createPrinter().printFile(transformed.transformed[0]), {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ESNext },
    }).outputText
    const exports: Record<string, any> = {}
    return compileFunction(`${code}; return { ...exports, ${declarations ? 'loadTappRuntimeModule' : ''} }`, ['exports', 'TestImport'])(exports, loader)
  } finally {
    transformed.dispose()
  }
}

test('motion import failures keep static fallback, then a later attempt coalesces and publishes readiness', async () => {
  let attempts = 0
  let finish!: (value: unknown) => void
  const api = loadSource(new URL('./lazyMotion.tsx', import.meta.url), () => {
    attempts++
    return attempts === 1 ? Promise.reject(new Error('offline')) : new Promise(resolve => { finish = resolve })
  })
  await api.ensureMotionReady()
  assert.equal(api.isMotionReady(), false)
  const first = api.ensureMotionReady()
  const second = api.ensureMotionReady()
  assert.equal(attempts, 2)
  assert.equal(first, second)
  finish({ motion: {}, AnimatePresence: {}, PresenceContext: {} })
  await first
  assert.equal(api.isMotionReady(), true)
  await api.ensureMotionReady()
  assert.equal(attempts, 2)
})

test('TAPP runtime import failures do not poison later retry attempts', async () => {
  let attempts = 0
  let finish!: (value: unknown) => void
  const api = loadSource(new URL('../hooks/useTappWidgets.ts', import.meta.url), () => {
    attempts++
    return attempts === 1 ? Promise.reject(new Error('offline')) : new Promise(resolve => { finish = resolve })
  }, ['runtimeModulePromise', 'loadTappRuntimeModule'])
  await assert.rejects(api.loadTappRuntimeModule(), /offline/)
  const first = api.loadTappRuntimeModule()
  const second = api.loadTappRuntimeModule()
  assert.equal(attempts, 2)
  assert.equal(first, second)
  const runtime = { getTappRuntime: () => ({}) }
  finish(runtime)
  assert.equal(await first, runtime)
  assert.equal(await api.loadTappRuntimeModule(), runtime)
  assert.equal(attempts, 2)
})

test('failed icon imports release pending names without an unhandled rejection or automatic retry loop', async () => {
  let attempts = 0
  let notifications = 0
  let finish!: (value: unknown) => void
  const api = loadSource(new URL('./namedIconCatalog.ts', import.meta.url), () => {
    attempts++
    return attempts === 1 ? Promise.reject(new Error('offline')) : new Promise(resolve => { finish = resolve })
  })
  const unwatch = api.subscribeNamedIcons(() => notifications++)
  try {
    api.requestNamedIcon('SiQq')
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(attempts, 1)
    assert.equal(notifications, 0)
    assert.equal(api.peekNamedIcon('SiQq'), undefined)
    api.requestNamedIcon('SiQq')
    api.requestNamedIcon('SiQq')
    assert.equal(attempts, 2)
    const icon = () => null
    finish({ getIconByName: () => icon })
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(api.peekNamedIcon('SiQq'), icon)
    assert.equal(notifications, 1)
    api.requestNamedIcon('SiQq')
    assert.equal(attempts, 2)
  } finally {
    unwatch()
  }
})
