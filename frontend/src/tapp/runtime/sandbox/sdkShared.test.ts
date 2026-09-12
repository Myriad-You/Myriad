import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { describe, it } from 'node:test'
import { runInNewContext } from 'node:vm'
import {
  DOM_HELPERS_CODE,
  FILE_DOWNLOAD_METHOD_CODE,
  generateKvNamespaceCode,
  generateSettingsNamespaceCode,
  SDK_FREEZE_TAPP_CODE,
} from './sdkShared.ts'

function createDomHelpers() {
  const context = {
    document: {
      createElement: () => ({
        textContent: '',
        attributes: {} as Record<string, string>,
        setAttribute(name: string, value: string) {
          this.attributes[name] = value
        },
      }),
    },
    Tapp: {} as { dom?: any },
  }
  context.Tapp.dom = runInNewContext(`(${DOM_HELPERS_CODE})`, context)
  return context.Tapp.dom
}

describe('generated DOM helpers', () => {
  it('blocks event handlers and executable URLs after normalization', () => {
    const dom = createDomHelpers()
    const element = dom.createElement('a')
    for (const [name, value] of [
      ['onClick', 'run()'],
      ['href', '  JaVaScRiPt:run()'],
      ['src', 'data:text/html,<script>run()</script>'],
      ['action', 'vbscript:run()'],
    ]) {
      dom.setAttribute(element, name, value)
    }
    assert.deepEqual(element.attributes, {})
    dom.setAttribute(element, 'href', 'https://example.com/')
    assert.equal(element.attributes.href, 'https://example.com/')
  })

  it('uses only own text properties, including falsy values and null prototypes', () => {
    const dom = createDomHelpers()
    assert.equal(dom.createElement('span', Object.create({ text: 'inherited' })).textContent, '')
    for (const text of ['', 0, false]) {
      const options = Object.assign(Object.create(null), { text, hasOwnProperty: null })
      assert.equal(dom.createElement('span', options).textContent, text)
    }
    assert.equal(dom.escapeHtml('<"&>'), '&lt;&quot;&amp;&gt;')
  })
})

describe('generated file download helper', () => {
  it('preserves binary bytes across chunks and concurrent downloads', async () => {
    const bytes = Uint8Array.from({ length: 0x10003 }, (_, index) => index % 256)
    const sent: Array<{ base64: string; filename: string; mimeType: string }> = []
    const file = runInNewContext(`({ ${FILE_DOWNLOAD_METHOD_CODE} })`, {
      btoa,
      fetch: async (url: string) => ({
        ok: true,
        arrayBuffer: async () => url === 'blob:large' ? bytes.buffer : new Uint8Array([0, 255]).buffer,
      }),
      sendRequest: async (api: string, method: string, [options]: typeof sent) => {
        assert.equal(api, 'file')
        assert.equal(method, 'download')
        sent.push({ ...options })
      },
    })
    await Promise.all([
      file.download({ url: 'blob:large', path: 'assets/large.bin' }, undefined, 'application/octet-stream'),
      file.download('blob:small', 'small.bin', 'application/octet-stream'),
    ])
    const large = sent.find((entry) => entry.filename === 'large.bin')!
    const small = sent.find((entry) => entry.filename === 'small.bin')!
    assert.deepEqual(new Uint8Array(Buffer.from(large.base64, 'base64')), bytes)
    assert.deepEqual(Iterator.from(Buffer.from(small.base64, 'base64')).toArray(), [0, 255])
    assert.equal(large.mimeType, 'application/octet-stream')
  })

  it('preserves falsy content and rejects unreadable blobs before sending', async () => {
    const sent: Array<{ content: unknown; filename?: string; mimeType?: string }> = []
    const file = runInNewContext(`({ ${FILE_DOWNLOAD_METHOD_CODE} })`, {
      fetch: async () => ({ ok: false }),
      sendRequest: async (_api: string, _method: string, [options]: typeof sent) => sent.push({ ...options }),
    })
    for (const content of ['', 0, false]) {
      await file.download(content, 'empty.txt', 'text/plain')
    }
    assert.deepEqual(sent.map((entry) => entry.content), ['', 0, false])
    await assert.rejects(file.download('blob:missing'), /Could not read blob/)
    assert.equal(sent.length, 3)
  })
})

describe('generateKvNamespaceCode', () => {
  it('keeps storage/shared/private on the same method surface', () => {
    for (const api of ['storage', 'shared', 'private'] as const) {
      const arrow = generateKvNamespaceCode(api, 'arrow')
      const fn = generateKvNamespaceCode(api, 'fn')
      for (const source of [arrow, fn]) {
        assert.match(source, new RegExp(`${RegExp.escape(api)}:\\s*\\{`))
        for (const method of [
          'get',
          'set',
          'remove',
          'keys',
          'getAll',
          'clear',
          'usage',
        ]) {
          assert.match(
            source,
            new RegExp(
              `sendRequest\\('${RegExp.escape(api)}', '${RegExp.escape(method)}'`,
            ),
          )
        }
        assert.match(
          source,
          new RegExp(`addEventListener\\('${RegExp.escape(api)}Changed'`),
        )
      }
      assert.match(arrow, /get:\s*\(k\)\s*=>/)
      assert.match(fn, /get:\s*function\(k\)/)
    }
  })
})

describe('generateSettingsNamespaceCode', () => {
  it('is a declared-key subset with onChanged', () => {
    const source = generateSettingsNamespaceCode('arrow')
    assert.match(source, /settings:\s*\{/)
    assert.match(source, /sendRequest\('settings', 'get'/)
    assert.match(source, /sendRequest\('settings', 'set'/)
    assert.match(source, /sendRequest\('settings', 'getAll'/)
    assert.match(source, /addEventListener\('settingsChanged'/)
    assert.doesNotMatch(source, /sendRequest\('settings', 'remove'/)
    assert.doesNotMatch(source, /sendRequest\('settings', 'clear'/)
    assert.doesNotMatch(source, /sendRequest\('settings', 'keys'/)
  })
})

describe('SDK_FREEZE_TAPP_CODE', () => {
  it('freezes window.Tapp and skips widgets/pages', () => {
    assert.match(SDK_FREEZE_TAPP_CODE, /skip = \{ widgets: 1, pages: 1 \}/)
    assert.match(SDK_FREEZE_TAPP_CODE, /\)\(window\.Tapp\)/)
    assert.doesNotMatch(SDK_FREEZE_TAPP_CODE, /\)\(Tapp\)/)
  })
})
