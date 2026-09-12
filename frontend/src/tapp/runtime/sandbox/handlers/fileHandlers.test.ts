import type { TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerFileHandlers } from './baseHandlers.ts'

const originalDocument = globalThis.document
const originalFetch = globalThis.fetch
const originalCreateObjectURL = URL.createObjectURL
const originalRevokeObjectURL = URL.revokeObjectURL
const downloads: Array<{ href: string; download: string }> = []
const fetches: string[] = []

afterEach(() => {
  globalThis.document = originalDocument
  globalThis.fetch = originalFetch
  URL.createObjectURL = originalCreateObjectURL
  URL.revokeObjectURL = originalRevokeObjectURL
  downloads.length = 0
  fetches.length = 0
})

function installDownloadDom() {
  const body = {
    appendChild(node: unknown) {
      return node
    },
    removeChild(node: unknown) {
      return node
    },
  }
  globalThis.document = {
    createElement() {
      const el = {
        href: '',
        download: '',
        style: { display: '' },
        parentNode: body,
        click() {
          downloads.push({ href: el.href, download: el.download })
        },
      }
      return el
    },
    body,
  } as unknown as Document
  URL.createObjectURL = () => 'blob:tapp-download'
  URL.revokeObjectURL = () => {}
}

class FakeBridge {
  readonly handlers = new Map<
    string,
    (message: TappMessage) => Promise<unknown>
  >()

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }
}

async function invoke(bridge: FakeBridge, args: unknown[] = []) {
  const handler = bridge.handlers.get('file.download')
  assert.ok(handler)
  return handler({
    type: 'request',
    id: 'file-1',
    action: 'file.download',
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerFileHandlers', { concurrency: false }, () => {
  it('rejects missing options, mixed sources, and arbitrary http urls', async () => {
    installDownloadDom()
    const bridge = new FakeBridge()
    registerFileHandlers(bridge as unknown as TappBridge)
    assert.deepEqual(await invoke(bridge, []), {
      success: false,
      error: 'Options required',
    })
    const mixed = await invoke(bridge, [
      { content: 'hi', url: '/api/brew/image-cache/ab/x.png' },
    ])
    assert.equal((mixed as { success: boolean }).success, false)
    const remote = await invoke(bridge, [
      { url: 'https://evil.example/secret.bin', filename: 'x.bin' },
    ])
    assert.equal((remote as { success: boolean }).success, false)
    assert.match(
      String((remote as { error?: string }).error),
      /image-cache|model3d/,
    )
    assert.equal(downloads.length, 0)
    assert.equal(fetches.length, 0)
  })

  it('rejects path traversal in filenames and host download urls', async () => {
    installDownloadDom()
    const bridge = new FakeBridge()
    registerFileHandlers(bridge as unknown as TappBridge)
    const traversal = await invoke(bridge, [
      { content: 'hi', filename: '../etc/passwd' },
    ])
    assert.equal((traversal as { success: boolean }).success, false)
    assert.match(String((traversal as { error?: string }).error), /filename/)
    const badHost = await invoke(bridge, [
      { url: '/api/brew/image-cache/aa/../x.png' },
    ])
    assert.equal((badHost as { success: boolean }).success, false)
    assert.equal(downloads.length, 0)
  })

  it('downloads inline content and allowlisted generated assets', async () => {
    installDownloadDom()
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      fetches.push(String(input))
      return {
        ok: true,
        status: 200,
        blob: async () => new Blob(['png'], { type: 'image/png' }),
      } as Response
    }) as typeof fetch
    const bridge = new FakeBridge()
    registerFileHandlers(bridge as unknown as TappBridge)
    const inline = await invoke(bridge, [
      { content: 'hello', filename: 'note.txt' },
    ])
    assert.deepEqual(inline, { success: true, data: { filename: 'note.txt' } })
    assert.equal(downloads[0]?.download, 'note.txt')

    const hash = `ab${'a'.repeat(62)}`
    const path = `/api/brew/image-cache/ab/${hash}.png`
    const generated = await invoke(bridge, [{ url: path, filename: 'cat.png' }])
    assert.deepEqual(generated, {
      success: true,
      data: { filename: 'cat.png' },
    })
    assert.deepEqual(fetches, [path])
    assert.equal(downloads[1]?.download, 'cat.png')
  })
})
