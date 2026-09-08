import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { Worker as ThreadWorker } from 'node:worker_threads'
import { writePsd } from 'ag-psd'
import { anime25DImportCopy } from './anime25dImportCopy'
import {
  decodeRigPsd,
  validateRigPsdHeader,
  validateRigPsdStructure,
} from './psdDecode'
import { importRigPsdInWorker } from './psdImportClient'

function header(): ArrayBuffer {
  const buffer = new ArrayBuffer(26)
  const view = new DataView(buffer)
  view.setUint32(0, 0x38425053)
  view.setUint16(4, 1)
  view.setUint16(12, 4)
  view.setUint32(14, 256)
  view.setUint32(18, 256)
  view.setUint16(22, 8)
  view.setUint16(24, 3)
  return buffer
}

test('PSD preflight rejects oversized/header/metadata hazards before pixel decoding', () => {
  validateRigPsdHeader(header())
  for (const [offset, value] of [
    [4, 2],
    [22, 16],
    [24, 4],
  ]) {
    const bytes = header()
    new DataView(bytes).setUint16(offset, value)
    assert.throws(() => validateRigPsdHeader(bytes), /psdSpecInvalid/)
  }
  const oversized = header()
  new DataView(oversized).setUint32(18, 100_000)
  assert.throws(() => validateRigPsdHeader(oversized), /psdSpecInvalid/)
  assert.throws(() => validateRigPsdHeader(new ArrayBuffer(10)))
  const layer = { left: 0, top: 0, right: 2048, bottom: 2048 }
  assert.throws(
    () =>
      validateRigPsdStructure({
        width: 256,
        height: 256,
        children: Array.from({ length: 9 }, () => ({ ...layer })),
      }),
    /psdTooLarge/,
  )
  let nested = { children: [layer] }
  for (let i = 0; i < 33; i += 1)
    nested = { children: [nested] } as typeof nested
  assert.throws(
    () =>
      validateRigPsdStructure({ width: 256, height: 256, children: [nested] }),
    /psdLayerCountInvalid/,
  )
})

function realPsdBytes(): ArrayBuffer {
  const pixels = new Uint8ClampedArray(16 * 16 * 4).fill(255)
  return writePsd(
    {
      width: 256,
      height: 256,
      children: [
        {
          name: 'group',
          children: [
            {
              name: 'face',
              left: 40,
              top: 30,
              imageData: { width: 16, height: 16, data: pixels },
            },
          ],
        },
      ],
    },
    { generateThumbnail: false },
  )
}

test('worker decoder round-trips real PSD bytes with nested layer pixels', () => {
  const pixels = new Uint8ClampedArray(16 * 16 * 4).fill(255)
  const bytes = realPsdBytes()
  const decoded = decodeRigPsd(bytes)
  const face = decoded.children![0].children![0]
  assert.equal(face.name, 'face')
  assert.equal(face.left, 40)
  assert.deepEqual(face.imageData!.data, pixels)
})

test('production worker bundles for the browser and reports localized parser errors from an isolated thread', async () => {
  // Reuse the test runner's bundled compiler, without a second build dependency.
  const { build } = createRequire(import.meta.resolve('tsx/package.json'))(
    'esbuild',
  )
  const bundle = await build({
    entryPoints: [new URL('./psdImport.worker.ts', import.meta.url).pathname],
    bundle: true,
    platform: 'browser',
    format: 'iife',
    write: false,
    metafile: true,
  })
  assert.ok(
    !Object.keys(bundle.metafile.inputs).some((path) =>
      path.includes('/i18n/'),
    ),
    'the worker receives import copy; it must not bundle UI language packs',
  )
  const worker = new ThreadWorker(
    `
    const { parentPort } = require('node:worker_threads');
    globalThis.postMessage = (data, options) => parentPort.postMessage(data, options?.transfer);
    ${bundle.outputFiles[0].text}
    parentPort.on('message', data => globalThis.onmessage({ data }));
  `,
    { eval: true },
  )
  try {
    const result = new Promise<{ error: string }>((resolve, reject) => {
      worker.once('message', resolve)
      worker.once('error', reject)
    })
    const bytes = header()
    const copy = {
      ...anime25DImportCopy(),
      psdSpecInvalid: '指定语言：PSD 无效',
    }
    worker.postMessage({ buffer: bytes, copy }, [bytes])
    assert.equal(bytes.byteLength, 0)
    assert.deepEqual(await result, { error: copy.psdSpecInvalid })
  } finally {
    await worker.terminate()
  }
})

test('worker lifecycle transfers bytes and terminates on success, failure and cancellation', async () => {
  for (const outcome of [
    'success',
    'error',
    'cancel',
    'messageerror',
    'throw',
  ] as const) {
    let terminated = 0
    let transferred = false
    const controller = new AbortController()
    const bytes = header()
    const worker = {
      onmessage: null,
      onerror: null,
      onmessageerror: null,
      terminate: () => {
        terminated += 1
      },
      postMessage: (value: unknown, transfer: Transferable[]) => {
        transferred =
          (value as { buffer: ArrayBuffer }).buffer === bytes &&
          transfer[0] === bytes
        if (outcome === 'throw') throw new Error('send failed')
      },
    } as unknown as Worker
    const stages: string[] = []
    const result = importRigPsdInWorker(
      request(bytes),
      controller.signal,
      (stage) => stages.push(stage),
      () => worker,
    )
    const late = worker.onmessage
    worker.onmessage?.({ data: { stage: 'validated' } } as MessageEvent)
    if (outcome === 'success') {
      worker.onmessage!({
        data: { prepared: { partCount: 16 } },
      } as MessageEvent)
    }
    if (outcome === 'error') worker.onerror!({} as ErrorEvent)
    if (outcome === 'messageerror') worker.onmessageerror!({} as MessageEvent)
    if (outcome === 'cancel') controller.abort()
    if (outcome === 'success') assert.equal((await result).partCount, 16)
    else await assert.rejects(result)
    late?.call(worker, {
      data: { prepared: { partCount: 99 } },
    } as MessageEvent)
    late?.call(worker, { data: { stage: 'packing' } } as MessageEvent)
    assert.deepEqual(stages, outcome === 'throw' ? [] : ['validated'])
    assert.equal(terminated, 1)
    assert.ok(transferred)
    assert.equal(worker.onmessage, null)
  }
  const aborted = new AbortController()
  aborted.abort()
  assert.throws(
    () =>
      importRigPsdInWorker(request(header()), aborted.signal, undefined, () => {
        throw new Error('must not create')
      }),
    { name: 'AbortError' },
  )
})

function request(buffer = header()) {
  return {
    buffer,
    sourceMasterAssetId: '/master.png',
    sourceMasterUrl: 'http://localhost/master.png',
    copy: anime25DImportCopy(),
  }
}

test('worker timeout releases resources and rejects late progress', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  let terminated = 0
  let progress = 0
  const worker = {
    postMessage() {},
    terminate() {
      terminated += 1
    },
    onmessage: null,
    onerror: null,
    onmessageerror: null,
  } as unknown as Worker
  const result = importRigPsdInWorker(
    request(),
    undefined,
    () => {
      progress += 1
    },
    () => worker,
  )
  const late = worker.onmessage!
  t.mock.timers.tick(60_000)
  await assert.rejects(result)
  late.call(worker, { data: { stage: 'packing' } } as MessageEvent)
  assert.equal(terminated, 1)
  assert.equal(progress, 0)
})

test('a falsy abort reason still rejects instead of resolving an empty import', async () => {
  const controller = new AbortController()
  const worker = { postMessage() {}, terminate() {} } as unknown as Worker
  const result = importRigPsdInWorker(
    request(),
    controller.signal,
    undefined,
    () => worker,
  )
  controller.abort(false)
  await assert.rejects(result, (reason) => reason === false)
})

test('cancellation during packing and throwing progress callbacks terminate before results publish', async () => {
  for (const throws of [false, true]) {
    let terminated = 0
    const controller = new AbortController()
    const worker = {
      postMessage() {},
      terminate() {
        terminated += 1
      },
      onmessage: null,
      onerror: null,
      onmessageerror: null,
    } as unknown as Worker
    const result = importRigPsdInWorker(
      request(),
      controller.signal,
      (stage) => {
        if (stage === 'packing') {
          if (throws) throw new Error('progress failed')
          controller.abort()
        }
      },
      () => worker,
    )
    const handler = worker.onmessage!
    handler.call(worker, { data: { stage: 'packing' } } as MessageEvent)
    handler.call(worker, {
      data: { prepared: { partCount: 99 } },
    } as MessageEvent)
    await assert.rejects(result)
    assert.equal(terminated, 1)
    assert.equal(worker.onmessage, null)
  }
})
