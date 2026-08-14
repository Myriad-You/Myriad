import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import vm from 'node:vm'
import { SANDBOXED_FETCH_INSTALL_SOURCE } from './assetUrlRewriter.ts'
import { generateFullSDK } from './sdkGenerator.ts'
import { generateSecurityWrapper } from './security.ts'

function install(windowLike: { fetch: typeof fetch }) {
  const context = vm.createContext({ window: windowLike })
  vm.runInContext(`${SANDBOXED_FETCH_INSTALL_SOURCE}\ninstallSandboxedFetch(window);`, context)
}

describe('sandboxed fetch (Three FileLoader shape)', () => {
  it('allows blob and data Request objects, rejects https', async () => {
    const seen: string[] = []
    const windowLike = {
      fetch: async (input: RequestInfo) => {
        const url = typeof input === 'string' ? input : input.url
        seen.push(url)
        return new Response('ok', { status: 200 })
      },
    }
    install(windowLike)

    const blobReq = { url: 'blob:https://opaque/cube' } as Request
    const blobResponse = await windowLike.fetch(blobReq)
    assert.equal(blobResponse.status, 200)

    const dataResponse = await windowLike.fetch('data:text/plain,hi')
    assert.equal(await dataResponse.text(), 'ok')

    await assert.rejects(
      () => windowLike.fetch(new Request('https://evil.example/model.glb')),
      /fetch disabled/,
    )
    assert.deepEqual(seen, ['blob:https://opaque/cube', 'data:text/plain,hi'])
  })

  it('is what the security wrapper actually installs', () => {
    const wrapper = generateSecurityWrapper('tok')
    assert.match(wrapper, /installSandboxedFetch\(window\)/)
    assert.match(wrapper, /isSandboxedFetchUrl/)
  })

  it('exposes getUrlMap and rewriteUrl on the Page SDK', () => {
    const sdk = generateFullSDK(
      {
        id: 'com.example.assets',
        manifest: {
          id: 'com.example.assets',
          name: 'Assets',
          version: '1.0.0',
          main: 'main.js',
          permissions: [],
          category: 'utility',
          assets: ['assets/cube.glb'],
        },
        status: 'running',
        installedAt: '2026-08-14T00:00:00Z',
        grantedPermissions: [],
        userRole: 'admin',
      },
      'session',
      'page',
    )
    assert.match(sdk, /getUrlMap:/)
    assert.match(sdk, /rewriteUrl:/)
    assert.match(sdk, /function rewriteAssetUrl/)
  })
})
