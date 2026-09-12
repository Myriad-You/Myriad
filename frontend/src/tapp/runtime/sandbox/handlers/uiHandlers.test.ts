import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerUIHandlers } from './baseHandlers.ts'

const originalDocument = globalThis.document
const clicks: Array<{ href: string; target: string; rel: string }> = []

afterEach(() => {
  globalThis.document = originalDocument
  clicks.length = 0
})

function installDocument() {
  const body = {
    appendChild(node: unknown) {
      return node
    },
    removeChild(node: unknown) {
      return node
    },
  }
  globalThis.document = {
    documentElement: {
      classList: { contains: () => false },
      lang: 'en-US',
    },
    createElement() {
      const el = {
        href: '',
        target: '',
        rel: '',
        style: { display: '' },
        click() {
          clicks.push({ href: el.href, target: el.target, rel: el.rel })
        },
      }
      return el
    },
    body,
  } as unknown as Document
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

  async getRuntimeGrant() {
    throw new Error('openUrl must not request a Runtime Grant')
  }
}

function instance(
  id: string,
  openUrls: Array<{ id: string; url: string; match?: string }> = [],
): TappInstance {
  return {
    id,
    manifest: {
      id,
      name: id,
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions: [],
      category: 'utility',
      openUrls,
    },
    status: 'running',
    installedAt: '2026-09-10T00:00:00Z',
    grantedPermissions: [],
    userRole: 'admin',
  }
}

async function invoke(
  bridge: FakeBridge,
  action: string,
  args: unknown[] = [],
) {
  const handler = bridge.handlers.get(action)
  assert.ok(handler, action)
  return handler({
    type: 'request',
    id: 'ui-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerUIHandlers openUrl', { concurrency: false }, () => {
  it('does not register openUrl in headless runtimes', () => {
    installDocument()
    const bridge = new FakeBridge()
    registerUIHandlers(
      bridge as unknown as TappBridge,
      instance('com.example.headless'),
      undefined,
      { headless: true },
    )
    assert.equal(bridge.handlers.has('ui.openUrl'), false)
    assert.equal(bridge.handlers.has('ui.listOpenUrls'), false)
    assert.equal(bridge.handlers.has('ui.getTheme'), true)
  })

  it('lists declared targets and opens only allowlisted ids', async () => {
    installDocument()
    const bridge = new FakeBridge()
    registerUIHandlers(
      bridge as unknown as TappBridge,
      instance('com.example.open-ok', [
        {
          id: 'docs',
          url: 'https://docs.example.com/guide/',
          match: 'prefix',
        },
      ]),
    )
    assert.deepEqual(await invoke(bridge, 'ui.listOpenUrls'), {
      success: true,
      data: [
        {
          id: 'docs',
          url: 'https://docs.example.com/guide/',
          match: 'prefix',
        },
      ],
    })
    const opened = await invoke(bridge, 'ui.openUrl', [
      { id: 'docs', path: 'install' },
    ])
    assert.deepEqual(opened, {
      success: true,
      data: {
        id: 'docs',
        url: 'https://docs.example.com/guide/install',
        match: 'prefix',
      },
    })
    assert.deepEqual(clicks, [
      {
        href: 'https://docs.example.com/guide/install',
        target: '_blank',
        rel: 'noopener noreferrer',
      },
    ])
  })

  it('rejects free-form URLs, undeclared ids, and path escape', async () => {
    installDocument()
    const bridge = new FakeBridge()
    registerUIHandlers(
      bridge as unknown as TappBridge,
      instance('com.example.open-deny', [
        { id: 'docs', url: 'https://docs.example.com/guide/', match: 'prefix' },
      ]),
    )
    const missing = await invoke(bridge, 'ui.openUrl', [])
    assert.equal((missing as { success: boolean }).success, false)

    const undeclared = await invoke(bridge, 'ui.openUrl', [
      { id: 'https://evil.example/' },
    ])
    assert.equal((undeclared as { success: boolean }).success, false)
    assert.match(String((undeclared as { error?: string }).error), /not declared/)

    const escape = await invoke(bridge, 'ui.openUrl', [
      { id: 'docs', path: '../evil' },
    ])
    assert.equal((escape as { success: boolean }).success, false)
    assert.equal(clicks.length, 0)
  })

  it('rate-limits openUrl per tapp without opening extras', async () => {
    installDocument()
    const bridge = new FakeBridge()
    registerUIHandlers(
      bridge as unknown as TappBridge,
      instance('com.example.open-rate', [
        { id: 'status', url: 'https://status.example.com/health', match: 'exact' },
      ]),
    )
    for (let i = 0; i < 8; i++) {
      const result = await invoke(bridge, 'ui.openUrl', [{ id: 'status' }])
      assert.equal((result as { success: boolean }).success, true, `hit ${i}`)
    }
    const blocked = await invoke(bridge, 'ui.openUrl', [{ id: 'status' }])
    assert.equal((blocked as { success: boolean }).success, false)
    assert.match(String((blocked as { error?: string }).error), /rate limit/)
    assert.equal(clicks.length, 8)
  })
})
