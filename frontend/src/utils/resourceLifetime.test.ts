import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'

function loadModule(name: string, globals = {}) {
  const source = readFileSync(new URL(name, import.meta.url), 'utf8').replaceAll('import.meta.env.DEV', 'false')
  const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ESNext } }).outputText
  const exports: Record<string, any> = {}
  runInNewContext(output, {
    exports, setTimeout, clearTimeout, console, AbortController,
    document: { readyState: 'complete', createElement: () => ({ getContext: () => ({}) }) },
    window: { setTimeout, clearTimeout }, Image: class {}, ...globals,
  })
  return exports
}
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

test('delayed tasks acquire at most three slots', async () => {
  const { globalResourceLoader: loader, LoadPriority } = loadModule('./resourceLoader.ts')
  loader.config.lowPriorityDelay = 5
  let active = 0
  let peak = 0
  const finishes: Array<() => void> = []
  for (let i = 0; i < 20; i++) {
    loader.addTask({ id: `${i}`, priority: LoadPriority.LOW, loader: () => {
      active++
      peak = Math.max(peak, active)
      return new Promise<void>((resolve) => finishes.push(() => { active--; resolve() }))
    } })
  }
  await sleep(25)
  try { assert.equal(peak, 3) } finally {
    loader.clear()
    for (const finish of finishes) finish()
  }
})

test('cancelled delayed tasks never run', async () => {
  // The loader's clock is held still between adding and cancelling: on the real one, a pause of the delay's
  // length between those two lines made the task due at once, and it ran before it could be cancelled.
  let now = 1000
  const { globalResourceLoader: loader, LoadPriority } = loadModule('./resourceLoader.ts', { Date: { now: () => now } })
  loader.config.lowPriorityDelay = 5
  const ran: string[] = []
  loader.addTask({ id: 'cancelled', priority: LoadPriority.LOW, loader: async () => { ran.push('cancelled') } })
  loader.cancelTask('cancelled')
  now += 1000
  loader.addTask({ id: 'later', priority: LoadPriority.HIGH, loader: async () => { ran.push('later') } })
  await sleep(25)
  try { assert.deepEqual(ran, ['later']) } finally { loader.clear() }
})

test('cancelling running tasks aborts without freeing occupied slots early', async () => {
  const { globalResourceLoader: loader, LoadPriority } = loadModule('./resourceLoader.ts')
  let signal: AbortSignal | undefined
  let finish!: () => void
  loader.addTask({ id: 'a', priority: LoadPriority.HIGH, loader: (value: AbortSignal) => {
    signal = value
    return new Promise<void>((resolve) => { finish = resolve })
  } })
  loader.cancelTask('a')
  try {
    assert.equal(signal?.aborted, true)
    assert.equal(loader.getStats().active, 1)
  } finally { finish(); await sleep(0); loader.clear() }
})

test('returned images release source and callbacks immediately', () => {
  const { imagePool } = loadModule('./objectPool.ts')
  const item = imagePool.acquire()
  item.img.src = 'https://example.test/large.png'
  item.img.onload = () => {}
  imagePool.release(item)
  try {
    assert.equal(item.img.src, '')
    assert.equal(item.img.onload, null)
  } finally { imagePool.destroy() }
})

test('cancelled running IDs accept a replacement after the old operation settles', async () => {
  const { globalResourceLoader: loader, LoadPriority } = loadModule('./resourceLoader.ts')
  let finish!: () => void
  let replacement = false
  loader.addTask({ id: 'same', priority: LoadPriority.HIGH, loader: () => new Promise<void>((resolve) => { finish = resolve }) })
  loader.cancelTask('same')
  loader.addTask({ id: 'same', priority: LoadPriority.HIGH, loader: async () => { replacement = true } })
  assert.equal(replacement, false)
  finish()
  await sleep(0)
  assert.equal(replacement, true)
  loader.clear()
})
