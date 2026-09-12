import type { TappInstance, TappPermission } from '../../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { PERMISSION_MAP } from '../permissionConfig.ts'
import { HEADLESS_DENIED_ACTIONS } from './capabilityProfiles.ts'
import {
  generateFullSDK,
  generateWidgetSDK,
} from './sdkGenerator.ts'

const KV_APIS = ['storage', 'shared', 'private'] as const
const KV_METHODS = [
  'get',
  'set',
  'remove',
  'keys',
  'getAll',
  'clear',
  'usage',
] as const
const WRITE_METHODS = new Set(['set', 'remove', 'clear'])

function makeInstance(permissions: string[] = []): TappInstance {
  return {
    id: 'com.example.sdk-runtime',
    manifest: {
      id: 'com.example.sdk-runtime',
      name: 'SDK Runtime',
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions: permissions as TappPermission[],
      category: 'utility',
    },
    status: 'running',
    installedAt: '2026-09-10T00:00:00Z',
    grantedPermissions: permissions as TappPermission[],
    userRole: 'admin',
  }
}

interface SdkSandbox {
  window: Record<string, unknown>
  posted: Array<Record<string, unknown>>
  deliver: (data: unknown) => void
  tapp: Record<string, unknown>
  themeToggles: Array<[string, boolean | undefined]>
  cssVars: Record<string, string>
}

function evalSdk(source: string): SdkSandbox {
  const posted: Array<Record<string, unknown>> = []
  const messageListeners: Array<(event: { source: unknown; data: unknown }) => void> =
    []
  const hostWindow = {
    postMessage(message: Record<string, unknown>) {
      posted.push(message)
    },
  }
  const themeToggles: Array<[string, boolean | undefined]> = []
  const cssVars: Record<string, string> = {}
  const classList = {
    toggle(name: string, on?: boolean) {
      themeToggles.push([name, on])
    },
  }
  const sandboxWindow: Record<string, unknown> = {
    parent: hostWindow,
    addEventListener(type: string, callback: unknown) {
      if (type === 'message') {
        messageListeners.push(
          callback as (event: { source: unknown; data: unknown }) => void,
        )
      }
    },
    _TAPP_I18N: {},
    _TAPP_LOCALE: 'en-US',
  }
  const sandboxDocument = {
    readyState: 'complete',
    addEventListener() {},
    createElement: () => ({ style: {}, appendChild() {} }),
    body: { style: {}, classList, offsetHeight: 0 },
    documentElement: {
      style: {
        setProperty(name: string, value: string) {
          cssVars[name] = value
        },
      },
      classList,
      lang: 'en-US',
    },
  }
  // eslint-disable-next-line no-new-func -- isolated SDK eval
  const run = new Function(
    'window',
    'document',
    'crypto',
    'setTimeout',
    'URL',
    'Blob',
    'atob',
    source,
  )
  run(
    sandboxWindow,
    sandboxDocument,
    globalThis.crypto,
    () => 0,
    URL,
    Blob,
    globalThis.atob,
  )
  return {
    window: sandboxWindow,
    posted,
    deliver(data) {
      for (const listener of messageListeners) {
        listener({ source: hostWindow, data })
      }
    },
    tapp: sandboxWindow.Tapp as Record<string, unknown>,
    themeToggles,
    cssVars,
  }
}

function kv(tapp: Record<string, unknown>, api: string) {
  return tapp[api] as Record<string, (...args: unknown[]) => Promise<unknown>>
}

async function roundTrip(
  sandbox: SdkSandbox,
  api: string,
  method: string,
  args: unknown[],
  data: unknown = null,
) {
  const pending = kv(sandbox.tapp, api)[method]!(...args)
  assert.equal(sandbox.posted.length, 1)
  const request = sandbox.posted[0]!
  assert.equal(request.type, 'request')
  assert.equal(request.action, `${api}.${method}`)
  assert.deepEqual(request.payload, { api, method, args })
  sandbox.deliver({
    type: 'response',
    id: request.id,
    payload: { success: true, data },
  })
  assert.equal(await pending, data)
  sandbox.posted.length = 0
}

describe('generated SDK runtime', () => {
  it('does not fall back to zh-CN when the current locale is missing', () => {
    const sandbox = evalSdk(generateFullSDK(makeInstance(), 'tok', 'page'))
    sandbox.window._TAPP_I18N = {
      'zh-CN': { hello: '你好' },
    }
    const i18n = sandbox.tapp.i18n as { t: (key: string) => string }
    assert.equal(i18n.t('hello'), 'hello')
    sandbox.deliver({
      type: 'event',
      action: 'locale:change',
      payload: 'zh-TW',
    })
    assert.equal(i18n.t('hello'), 'hello')
    sandbox.window._TAPP_I18N = {
      'zh-CN': { hello: '你好' },
      'en-US': { hello: 'Hello' },
    }
    assert.equal(i18n.t('hello'), 'Hello')
  })

  it('does not expose a readable credentials namespace', () => {
    const instance = makeInstance()
    const page = generateFullSDK(instance, 'tok', 'page')
    const widget = generateWidgetSDK(instance, 'tok')
    for (const source of [page, widget]) {
      assert.doesNotMatch(source, /\n\s+credentials:\s*\{/)
      assert.doesNotMatch(source, /sendRequest\('credentials'/)
    }
  })

  it('sends Tapp.api execute/list over the bridge without a credential payload', async () => {
    const instance = makeInstance()
    const pageSource = generateFullSDK(instance, 'session-token', 'page')
    const widgetSource = generateWidgetSDK(instance, 'session-token')
    assert.match(pageSource, /sendRequest\('api', 'execute', \[name, params\]\)/)
    assert.match(widgetSource, /sendRequest\('api', 'execute', \[name, params\]\)/)
    assert.equal(PERMISSION_MAP.get('api.execute'), 'public')
    assert.equal(PERMISSION_MAP.get('api.list'), 'public')

    for (const sandbox of [evalSdk(pageSource), evalSdk(widgetSource)]) {
      const api = sandbox.tapp.api as ((
        name: string,
        params?: unknown,
      ) => Promise<unknown>) & { list: () => Promise<unknown> }
      const pending = api('weather', { q: 'tokyo' })
      assert.equal(sandbox.posted.length, 1)
      const request = sandbox.posted[0]!
      assert.equal(request.action, 'api.execute')
      assert.equal(request._sessionToken, 'session-token')
      assert.deepEqual(request.payload, {
        api: 'api',
        method: 'execute',
        args: ['weather', { q: 'tokyo' }],
      })
      assert.equal(JSON.stringify(request).includes('top-secret'), false)
      sandbox.deliver({
        type: 'response',
        id: request.id,
        payload: { success: true, data: { echo: '[REDACTED]' } },
      })
      assert.deepEqual(await pending, { echo: '[REDACTED]' })

      sandbox.posted.length = 0
      const listed = api.list()
      assert.equal(sandbox.posted[0]!.action, 'api.list')
      assert.deepEqual(sandbox.posted[0]!.payload, {
        api: 'api',
        method: 'list',
        args: [],
      })
      sandbox.deliver({
        type: 'response',
        id: sandbox.posted[0]!.id,
        payload: {
          success: true,
          data: [{ name: 'weather', access: 'protected', type: 'http' }],
        },
      })
      assert.deepEqual(await listed, [
        { name: 'weather', access: 'protected', type: 'http' },
      ])
    }
  })

  it('sends openUrl/file/assets/ai over the bridge without host secrets', async () => {
    const sandbox = evalSdk(
      generateFullSDK(makeInstance(), 'session-token', 'page'),
    )
    const ui = sandbox.tapp.ui as {
      openUrl: (req: unknown) => Promise<unknown>
    }
    const file = sandbox.tapp.file as {
      download: (options: unknown) => Promise<unknown>
    }
    const assets = sandbox.tapp.assets as {
      get: (path: string) => Promise<unknown>
    }
    const ai = sandbox.tapp.ai as {
      tasks: { create: (request: unknown) => Promise<unknown> }
    }

    const openPending = ui.openUrl({ id: 'docs', path: 'install' })
    assert.equal(sandbox.posted[0]!.action, 'ui.openUrl')
    assert.deepEqual(sandbox.posted[0]!.payload, {
      api: 'ui',
      method: 'openUrl',
      args: [{ id: 'docs', path: 'install' }],
    })
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: {
        success: true,
        data: { url: 'https://docs.example.com/guide/install' },
      },
    })
    await openPending
    sandbox.posted.length = 0

    const filePending = file.download({
      content: 'hello',
      filename: 'note.txt',
    })
    assert.equal(sandbox.posted[0]!.action, 'file.download')
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: { success: true, data: { filename: 'note.txt' } },
    })
    await filePending
    sandbox.posted.length = 0

    const assetPending = assets.get('assets/icon.png')
    assert.equal(sandbox.posted[0]!.action, 'assets.get')
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: {
        success: true,
        data: { path: 'assets/icon.png', base64: 'QQ==' },
      },
    })
    await assetPending
    sandbox.posted.length = 0

    const aiPending = ai.tasks.create({
      operation: 'generate',
      prompt: 'hi',
    })
    assert.equal(sandbox.posted[0]!.action, 'ai.tasks.create')
    assert.equal(sandbox.posted[0]!._sessionToken, 'session-token')
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: { success: true, data: { id: 'task-1' } },
    })
    assert.deepEqual(await aiPending, { id: 'task-1' })
    assert.equal(PERMISSION_MAP.get('ui.openUrl'), 'ui:openUrl')
    assert.equal(PERMISSION_MAP.get('file.download'), 'public')
    assert.equal(PERMISSION_MAP.get('assets.get'), 'public')
    assert.equal(PERMISSION_MAP.get('ai.tasks.create'), 'public')
    assert.equal(PERMISSION_MAP.get('agent.v2.accept'), 'public')
  })

  it('freezes with window.Tapp, not a bare identifier', () => {
    const instance = makeInstance()
    const page = generateFullSDK(instance, 'session-token', 'page')
    const widget = generateWidgetSDK(instance, 'session-token')
    for (const source of [page, widget]) {
      assert.match(source, /\)\(window\.Tapp\)/)
      assert.doesNotMatch(source, /\)\(Tapp\);/)
    }
    assert.match(widget, /value:\s*window\.Tapp/)
  })

  it('exposes the four KV surfaces on page, headless, and widget', () => {
    const instance = makeInstance()
    const page = evalSdk(generateFullSDK(instance, 'tok', 'page'))
    const headless = evalSdk(generateFullSDK(instance, 'tok', 'headless'))
    const widget = evalSdk(generateWidgetSDK(instance, 'tok'))
    for (const sandbox of [page, headless, widget]) {
      for (const api of [...KV_APIS, 'settings']) {
        const ns = sandbox.tapp[api] as Record<string, unknown>
        assert.equal(typeof ns.get, 'function', `${api}.get`)
        assert.equal(typeof ns.set, 'function', `${api}.set`)
        assert.equal(typeof ns.onChanged, 'function', `${api}.onChanged`)
        assert.equal(Object.isFrozen(ns), true, `${api} frozen`)
      }
      for (const api of KV_APIS) {
        const ns = sandbox.tapp[api] as Record<string, unknown>
        for (const method of KV_METHODS) {
          assert.equal(typeof ns[method], 'function', `${api}.${method}`)
        }
      }
      assert.equal(Object.isFrozen(sandbox.tapp), true)
      assert.equal(sandbox.tapp.credentials, undefined)
      const descriptor = Object.getOwnPropertyDescriptor(sandbox.window, 'Tapp')
      assert.equal(descriptor?.writable, false)
      assert.equal(descriptor?.configurable, false)
      assert.throws(() => {
        sandbox.window.Tapp = {}
      }, TypeError)
    }
  })

  it('keeps widgets/pages extensible on page and widget, strips them in headless', () => {
    const instance = makeInstance()
    const page = evalSdk(generateFullSDK(instance, 'tok', 'page')).tapp
    const widget = evalSdk(generateWidgetSDK(instance, 'tok')).tapp
    const headless = evalSdk(generateFullSDK(instance, 'tok', 'headless')).tapp
    for (const tapp of [page, widget]) {
      const widgets = tapp.widgets as Record<string, unknown>
      const pages = tapp.pages as Record<string, unknown>
      widgets.demo = { render() {} }
      pages.home = { render() {} }
      assert.equal(typeof (widgets.demo as { render: unknown }).render, 'function')
      assert.equal(typeof (pages.home as { render: unknown }).render, 'function')
      assert.equal(Object.isFrozen(widgets), false)
      assert.equal(Object.isFrozen(pages), false)
    }
    assert.equal(headless.widgets, undefined)
    assert.equal(headless.pages, undefined)
  })

  it('sends storage/shared/private/settings requests with session token, not a grant', async () => {
    const instance = makeInstance()
    const sandbox = evalSdk(generateFullSDK(instance, 'session-token', 'page'))
    await roundTrip(sandbox, 'storage', 'get', ['ready'], { v: 1 })
    await roundTrip(sandbox, 'shared', 'set', ['posts', [1]], null)
    await roundTrip(sandbox, 'private', 'keys', [], ['token'])
    await roundTrip(sandbox, 'settings', 'getAll', [], { theme: 'dark' })
    await roundTrip(sandbox, 'private', 'usage', [], { used: 1, quota: 8 })
    const widget = evalSdk(generateWidgetSDK(instance, 'widget-token'))
    const pending = kv(widget.tapp, 'private').get!('secret')
    assert.equal(widget.posted[0]!._sessionToken, 'widget-token')
    assert.equal(widget.posted[0]!.action, 'private.get')
    widget.deliver({
      type: 'response',
      id: widget.posted[0]!.id,
      payload: { success: true, data: null },
    })
    assert.equal(await pending, null)
    assert.equal(
      sandbox.posted.length === 0 ||
        sandbox.posted.every((message) => message._sessionToken === 'session-token'),
      true,
    )
  })

  it('rejects invalid KV keys in the sandbox before postMessage', async () => {
    const sandbox = evalSdk(
      generateFullSDK(makeInstance(), 'tok', 'page'),
    )
    for (const api of KV_APIS) {
      await assert.rejects(async () => kv(sandbox.tapp, api).get!('../x'), /path/)
      await assert.rejects(async () => kv(sandbox.tapp, api).get!(''), /non-empty/)
      await assert.rejects(async () => kv(sandbox.tapp, api).get!('.hidden'), /dot/)
    }
    assert.equal(sandbox.posted.length, 0)
  })

  it('delivers onChanged payloads without values for every KV namespace', () => {
    const sandbox = evalSdk(
      generateFullSDK(makeInstance(), 'tok', 'page'),
    )
    const seen: Array<{ api: string; payload: unknown }> = []
    for (const api of [...KV_APIS, 'settings']) {
      const ns = sandbox.tapp[api] as {
        onChanged: (cb: (payload: unknown) => void) => () => void
      }
      ns.onChanged((payload) => {
        seen.push({ api, payload })
      })
    }
    sandbox.deliver({
      type: 'event',
      action: 'privateChanged',
      payload: { key: 'token', operation: 'set' },
    })
    sandbox.deliver({
      type: 'event',
      action: 'sharedChanged',
      payload: { key: 'posts', operation: 'remove' },
    })
    sandbox.deliver({
      type: 'event',
      action: 'storageChanged',
      payload: { operation: 'clear' },
    })
    sandbox.deliver({
      type: 'event',
      action: 'settingsChanged',
      payload: { key: 'theme', operation: 'set' },
    })
    assert.deepEqual(seen, [
      { api: 'private', payload: { key: 'token', operation: 'set' } },
      { api: 'shared', payload: { key: 'posts', operation: 'remove' } },
      { api: 'storage', payload: { operation: 'clear' } },
      { api: 'settings', payload: { key: 'theme', operation: 'set' } },
    ])
    for (const row of seen) {
      assert.equal(
        Object.hasOwn(row.payload as object, 'value'),
        false,
      )
    }
  })

  it('maps every generated KV sendRequest to storage:read or storage:write', () => {
    const instance = makeInstance()
    const page = generateFullSDK(instance, 'tok', 'page')
    const widget = generateWidgetSDK(instance, 'tok')
    for (const source of [page, widget]) {
      const actions = new Set(
        Iterator.from(
          source.matchAll(/sendRequest\(\s*'([^']+)',\s*'([^']+)'/g),
        ).map(([, namespace, operation]) => `${namespace}.${operation}`),
      )
      for (const api of KV_APIS) {
        for (const method of KV_METHODS) {
          const action = `${api}.${method}`
          assert.equal(actions.has(action), true, `${action} in SDK`)
          assert.equal(
            PERMISSION_MAP.get(action),
            WRITE_METHODS.has(method) ? 'storage:write' : 'storage:read',
            action,
          )
        }
        assert.equal(PERMISSION_MAP.has(`${api}.onChanged`), false)
      }
      assert.equal(PERMISSION_MAP.get('settings.get'), 'storage:read')
      assert.equal(PERMISSION_MAP.get('settings.set'), 'storage:write')
      assert.equal(PERMISSION_MAP.get('settings.getAll'), 'storage:read')
      assert.equal(PERMISSION_MAP.has('settings.onChanged'), false)
    }
    for (const action of HEADLESS_DENIED_ACTIONS) {
      assert.equal(action.startsWith('storage.'), false, action)
      assert.equal(action.startsWith('shared.'), false, action)
      assert.equal(action.startsWith('private.'), false, action)
      assert.equal(action.startsWith('settings.'), false, action)
    }
  })

  it('propagates host error codes and lets onChanged unsubscribe', async () => {
    const sandbox = evalSdk(generateFullSDK(makeInstance(), 'tok', 'page'))
    const pending = kv(sandbox.tapp, 'private').get!('token')
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: {
        success: false,
        error: 'Permission denied: Missing permission: storage:read',
        code: 'PERMISSION_DENIED',
      },
    })
    await assert.rejects(
      pending,
      (error: unknown) => {
        assert.ok(error instanceof Error)
        assert.equal(
          (error as Error & { code?: string }).code,
          'PERMISSION_DENIED',
        )
        return true
      },
    )

    const hits: unknown[] = []
    const off = (
      sandbox.tapp.private as {
        onChanged: (cb: (payload: unknown) => void) => () => void
      }
    ).onChanged((payload) => hits.push(payload))
    sandbox.deliver({
      type: 'event',
      action: 'privateChanged',
      payload: { key: 'token', operation: 'set' },
    })
    off()
    sandbox.deliver({
      type: 'event',
      action: 'privateChanged',
      payload: { key: 'token', operation: 'remove' },
    })
    assert.deepEqual(hits, [{ key: 'token', operation: 'set' }])
  })

  it('delivers privateChanged on Widget SDK and applies theme chrome', () => {
    const widget = evalSdk(generateWidgetSDK(makeInstance(), 'tok'))
    const hits: unknown[] = []
    ;(
      widget.tapp.private as {
        onChanged: (cb: (payload: unknown) => void) => () => void
      }
    ).onChanged((payload) => hits.push(payload))
    widget.deliver({
      type: 'event',
      action: 'privateChanged',
      payload: { key: 'token', operation: 'set' },
    })
    assert.deepEqual(hits, [{ key: 'token', operation: 'set' }])

    widget.deliver({ type: 'event', action: 'theme:change', payload: 'dark' })
    assert.deepEqual(widget.themeToggles, [
      ['dark', true],
      ['light', false],
    ])
    assert.equal(widget.cssVars['--tapp-text'], '#f3f4f6')
    widget.deliver({
      type: 'event',
      action: 'primaryColor:change',
      payload: '#94a3b8',
    })
    assert.equal(widget.cssVars['--tapp-primary'], '#94a3b8')
  })

  it('accepts report.create as an object or as positional fields', async () => {
    const sandbox = evalSdk(generateFullSDK(makeInstance(), 'tok', 'page'))
    const report = sandbox.tapp.report as {
      create: (...args: unknown[]) => Promise<unknown>
    }
    const objectPending = report.create({
      title: 'Weekly',
      reportType: 'custom',
      content: { n: 1 },
    })
    assert.deepEqual(sandbox.posted[0]!.payload, {
      api: 'report',
      method: 'create',
      args: [
        { title: 'Weekly', reportType: 'custom', content: { n: 1 } },
      ],
    })
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: { success: true, data: { id: 'r1' } },
    })
    assert.deepEqual(await objectPending, { id: 'r1' })
    sandbox.posted.length = 0

    const positionalPending = report.create('Weekly', 'custom', { n: 1 }, { k: 1 })
    assert.deepEqual(sandbox.posted[0]!.payload, {
      api: 'report',
      method: 'create',
      args: [
        {
          title: 'Weekly',
          reportType: 'custom',
          content: { n: 1 },
          metadata: { k: 1 },
        },
      ],
    })
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: { success: true, data: { id: 'r2' } },
    })
    assert.deepEqual(await positionalPending, { id: 'r2' })
  })

  it('still applies theme chrome when a themeChange listener throws', () => {
    const page = evalSdk(generateFullSDK(makeInstance(), 'tok', 'page'))
    const widget = evalSdk(generateWidgetSDK(makeInstance(), 'tok'))
    for (const sandbox of [page, widget]) {
      const ui = sandbox.tapp.ui as {
        onThemeChange: (cb: (payload: unknown) => void) => void
      }
      ui.onThemeChange(() => {
        throw new Error('listener')
      })
      sandbox.deliver({
        type: 'event',
        action: 'theme:change',
        payload: 'dark',
      })
      assert.equal(sandbox.cssVars['--tapp-text'], '#f3f4f6')
    }
  })

  it('runs Widget onDestroy when the host emits lifecycle:destroy', () => {
    const widget = evalSdk(generateWidgetSDK(makeInstance(), 'tok'))
    let destroyed = 0
    ;(
      widget.tapp.lifecycle as { onDestroy: (cb: () => void) => void }
    ).onDestroy(() => {
      destroyed += 1
    })
    widget.deliver({
      type: 'event',
      action: 'lifecycle:destroy',
      payload: null,
    })
    widget.deliver({
      type: 'event',
      action: 'lifecycle:destroy',
      payload: null,
    })
    assert.equal(destroyed, 1)
  })

  it('dispatches Tapp.on("theme:change") on Widget as well as Page', () => {
    const page = evalSdk(generateFullSDK(makeInstance(), 'tok', 'page'))
    const widget = evalSdk(generateWidgetSDK(makeInstance(), 'tok'))
    for (const sandbox of [page, widget]) {
      const hits: unknown[] = []
      const on = sandbox.tapp.on as (
        event: string,
        cb: (payload: unknown) => void,
      ) => void
      on('theme:change', (payload) => hits.push(payload))
      sandbox.deliver({
        type: 'event',
        action: 'theme:change',
        payload: 'dark',
      })
      assert.deepEqual(hits, ['dark'])
    }
  })

  it('keeps the spectrum stream enabled until the last subscriber leaves', () => {
    const instance = makeInstance(['media:read', 'media:control'])
    const page = evalSdk(generateFullSDK(instance, 'tok', 'page'))
    const widget = evalSdk(generateWidgetSDK(instance, 'tok'))
    for (const sandbox of [page, widget]) {
      sandbox.posted.length = 0
      const media = sandbox.tapp.media as {
        onSpectrum: (cb: (payload: unknown) => void) => () => void
      }
      const off1 = media.onSpectrum(() => {})
      const off2 = media.onSpectrum(() => {})
      const enables = sandbox.posted.filter(
        (message) => message.action === 'media.spectrumStream',
      )
      assert.equal(enables.length, 1)
      assert.deepEqual((enables[0]!.payload as { args: unknown[] }).args, [
        { enabled: true },
      ])
      sandbox.posted.length = 0
      off1()
      assert.equal(sandbox.posted.length, 0)
      off2()
      assert.equal(sandbox.posted[0]!.action, 'media.spectrumStream')
      assert.deepEqual(
        (sandbox.posted[0]!.payload as { args: unknown[] }).args,
        [{ enabled: false }],
      )
      off2()
      assert.equal(
        sandbox.posted.filter(
          (message) => message.action === 'media.spectrumStream',
        ).length,
        1,
      )
    }
  })

  it('fetches asset URLs in parallel and shares locale fallback with Widget', async () => {
    const pageSource = generateFullSDK(makeInstance(), 'tok', 'page')
    const widgetSource = generateWidgetSDK(makeInstance(), 'tok')
    assert.match(pageSource, /document\.documentElement\.lang/)
    assert.match(widgetSource, /\|\|\s*'en-US'/)
    assert.match(pageSource, /\|\|\s*'en-US'/)
    assert.doesNotMatch(widgetSource, /TappWidgetSDK/)
    assert.match(widgetSource, /lifecycle:destroy/)

    const sandbox = evalSdk(pageSource)
    const pending = (
      sandbox.tapp.assets as {
        getUrlMap: () => Promise<Record<string, string>>
      }
    ).getUrlMap()
    assert.equal(sandbox.posted[0]!.action, 'assets.list')
    sandbox.deliver({
      type: 'response',
      id: sandbox.posted[0]!.id,
      payload: { success: true, data: ['a.png', 'b.png'] },
    })
    await Promise.resolve()
    const gets = sandbox.posted.filter(
      (message) => message.action === 'assets.get',
    )
    assert.equal(gets.length, 2)
    for (const request of gets) {
      sandbox.deliver({
        type: 'response',
        id: request.id,
        payload: {
          success: true,
          data: {
            path: (request.payload as { args: string[] }).args[0],
            mimeType: 'image/png',
            size: 1,
            base64: 'QQ==',
          },
        },
      })
    }
    const map = await pending
    assert.equal(typeof map['a.png'], 'string')
    assert.equal(typeof map['b.png'], 'string')
  })

  it('maps host animationLevel:change onto onLevelChange and replays it', () => {
    const sandbox = evalSdk(generateFullSDK(makeInstance(), 'tok', 'page'))
    const hits: unknown[] = []
    const animation = sandbox.tapp.animation as {
      onLevelChange: (cb: (level: unknown) => void) => () => void
    }
    sandbox.deliver({
      type: 'event',
      action: 'animationLevel:change',
      payload: 'light',
    })
    animation.onLevelChange((level) => hits.push(level))
    assert.deepEqual(hits, ['light'])
    sandbox.deliver({
      type: 'event',
      action: 'animationLevel:change',
      payload: 'exlight',
    })
    assert.deepEqual(hits, ['light', 'exlight'])
  })

  it('omits model3d on Widget and rejects widget-only writes without a host call', async () => {
    const widget = evalSdk(
      generateWidgetSDK(makeInstance(['platform:read', 'report:read']), 'tok'),
    )
    assert.equal(widget.tapp.model3d, undefined)
    const platform = widget.tapp.platform as {
      addItem: (data: unknown) => Promise<unknown>
    }
    const report = widget.tapp.report as {
      create: (data: unknown) => Promise<unknown>
    }
    widget.posted.length = 0
    await assert.rejects(platform.addItem({}), /not available in the widget sandbox/)
    await assert.rejects(report.create({}), /not available in the widget sandbox/)
    assert.equal(widget.posted.length, 0)
  })

  it('throws when denied Widget media listeners are registered', () => {
    const widget = evalSdk(generateWidgetSDK(makeInstance([]), 'tok'))
    const media = widget.tapp.media as {
      onStateChange: (cb: () => void) => () => void
    }
    assert.throws(() => media.onStateChange(() => {}), /media:read/)
  })

  it('subscribes before listening for scheduler tasks', () => {
    const source = generateFullSDK(makeInstance(), 'tok', 'page')
    assert.match(
      source,
      /sendRequest\('scheduler', 'subscribe'[\s\S]{0,500}addEventListener\('schedulerTask'/,
    )
  })
})
