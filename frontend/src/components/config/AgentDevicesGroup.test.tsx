import type { ReactNode } from 'react'
import type { SyncDevice, SyncDeviceList } from '../../services/agent/agentApi'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import zh from '../../i18n/config.zh-CN.json'
import { buildDeviceBindUri } from '../../utils/deviceBindQr'

const require = createRequire(import.meta.url)
const { JSDOM } = require(
  require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }),
)
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

function makeDeviceList(siteUrl: string | null): SyncDeviceList {
  return {
    devices: [],
    host: {
      holder: 'site',
      holderName: null,
      lineage: 'lin_12345678',
      epoch: 1,
      state: 'live',
      move: null,
    },
    siteUrl,
    limit: 10,
  }
}

function makeDevice(name: string): SyncDevice {
  return {
    id: 'dev_test_1',
    name,
    tokenHint: 'mys_01234567',
    createdAt: '2026-11-26T00:00:00Z',
    lastSeenAt: null,
    revokedAt: null,
    revokedReason: null,
    stale: false,
  }
}

async function mountDevicesGroup(options: {
  origin: string
  listSiteUrl: string | null
  addSiteUrl: string | null
}) {
  const dom = new JSDOM('<div id="root"></div>', { url: options.origin })
  const globals = {
    window: dom.window,
    document: dom.window.document,
    IS_REACT_ACT_ENVIRONMENT: true,
  }
  const previous = new Map(
    Object.keys(globals).map(key => [
      key,
      Object.getOwnPropertyDescriptor(globalThis, key),
    ]),
  )
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const { act, createElement } = require('react') as typeof import('react')
  const { createRoot } = require('react-dom/client') as typeof import('react-dom/client')

  const bundle = await build({
    entryPoints: [new URL('./AgentDevicesGroup.tsx', import.meta.url).pathname],
    bundle: true,
    write: false,
    platform: 'node',
    format: 'cjs',
    packages: 'external',
    define: { 'import.meta.env': '{}' },
    loader: { '.css': 'empty' },
    plugins: [
      {
        name: 'boundaries',
        setup(builder) {
          builder.onResolve(
            {
              filter:
                /(@lib\/icons|contexts\/I18nContext|services\/agent|services\/api|utils\/toastManager|utils\/userFacingError|\/settings)$/,
            },
            ({ path }) => ({ path, external: true }),
          )
        },
      },
    ],
  })

  const token = 'mys_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg'
  let currentList = makeDeviceList(options.listSiteUrl)

  const fill = (text: string, values: Record<string, string | number> = {}) =>
    text.replace(/\{(\w+)\}/g, (_, key: string) => String(values[key] ?? ''))

  const mockRequire = (path: string) => {
    if (path.includes('I18nContext')) {
      return {
        useConfigI18n: () => ({
          t: { config: zh, common: { cancel: '取消', loading: '加载中...' } },
          locale: 'zh-CN',
          format: fill,
        }),
      }
    }
    if (path.endsWith('services/agent')) {
      return {
        agentService: {
          listSyncDevices: async () => currentList,
          addSyncDevice: async (name: string) => {
            const device = makeDevice(name)
            currentList = {
              ...currentList,
              devices: [device],
            }
            return { device, token, siteUrl: options.addSiteUrl }
          },
        },
      }
    }
    if (path.endsWith('services/api')) {
      return { ApiError: class ApiError extends Error {} }
    }
    if (path.endsWith('toastManager')) {
      return {
        showStickyToast: (opts: { message: string }) => {
          throw new Error(`Unexpected toast: ${opts.message}`)
        },
      }
    }
    if (path.endsWith('userFacingError')) {
      return { userFacingError: (_e: unknown, fallback: string) => fallback }
    }
    if (path.endsWith('/settings')) {
      return {
        SettingGroup: ({
          title,
          children,
        }: {
          title: string
          children: ReactNode
        }) => createElement('section', { 'data-title': title }, children),
        InfoActionCard: ({
          title,
          fields = [],
          actions = [],
          children,
        }: {
          title?: ReactNode
          fields?: Array<{ key: string, label: ReactNode, value: ReactNode }>
          actions?: Array<{ key: string, label: ReactNode, onClick: () => void }>
          children?: ReactNode
        }) =>
          createElement(
            'div',
            { 'data-testid': 'info-card' },
            createElement('h4', null, title),
            children,
            createElement(
              'dl',
              null,
              fields.map(field =>
                createElement(
                  'div',
                  { key: field.key, 'data-field': field.key },
                  createElement('dt', null, field.label),
                  createElement('dd', null, field.value),
                ),
              ),
            ),
            createElement(
              'div',
              null,
              actions.map(action =>
                createElement(
                  'button',
                  {
                    key: action.key,
                    type: 'button',
                    'data-action': action.key,
                    onClick: action.onClick,
                  },
                  action.label,
                ),
              ),
            ),
          ),
        InputItem: ({
          itemKey,
          label,
          value,
          onChange,
        }: {
          itemKey: string
          label: string
          value: string
          onChange: (value: string) => void
        }) =>
          createElement(
            'label',
            null,
            label,
            createElement('input', {
              'data-item': itemKey,
              value,
              onChange: (e: { target: { value: string } }) =>
                onChange(e.target.value),
            }),
          ),
        ManagedList: ({ form }: { form: ReactNode }) =>
          createElement('div', { 'data-testid': 'managed-list' }, form),
        SettingsButton: ({
          children,
          onClick,
          disabled,
        }: {
          children: ReactNode
          onClick?: () => void
          disabled?: boolean
        }) =>
          createElement(
            'button',
            { type: 'button', disabled, onClick },
            children,
          ),
      }
    }
    if (path === '@lib/icons') {
      return {
        LuDownload: () => null,
        LuMonitor: () => null,
        LuPlus: () => null,
        LuQrCode: () => null,
        LuUpload: () => null,
      }
    }
    return require(path)
  }

  const mod = { exports: {} as typeof import('./AgentDevicesGroup') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports'])(
    mockRequire,
    mod,
    mod.exports,
  )

  const container = dom.window.document.getElementById('root')!
  const root = createRoot(container)
  await act(async () => {
    root.render(createElement(mod.exports.AgentDevicesGroup))
  })

  const setInputValue = async (name: string) => {
    const input = container.querySelector(
      'input[data-item="agent_device_name"]',
    ) as HTMLInputElement
    const setter = Object.getOwnPropertyDescriptor(
      dom.window.HTMLInputElement.prototype,
      'value',
    )!.set!
    await act(async () => {
      setter.call(input, name)
      input.dispatchEvent(new dom.window.Event('input', { bubbles: true }))
    })
  }

  const submitAdd = async () => {
    const createBtn = [...container.querySelectorAll('button')].find(btn =>
      btn.textContent?.includes(zh.agentDevicesCreate),
    )!
    assert.equal(createBtn.disabled, false, 'create button should be enabled')
    await act(async () => {
      createBtn.click()
      await new Promise(resolve => setTimeout(resolve, 0))
    })
  }

  return {
    act,
    dom,
    container,
    token,
    setInputValue,
    submitAdd,
    cleanup: async () => {
      await act(async () => {
        root.unmount()
      })
      for (const [key, descriptor] of previous) {
        if (descriptor) Object.defineProperty(globalThis, key, descriptor)
        else delete (globalThis as Record<string, unknown>)[key]
      }
    },
  }
}

test('renders QR code, site URL, token, and bind URI when adding a device, and clears on close', async () => {
  const ctx = await mountDevicesGroup({
    origin: 'http://localhost:1102',
    listSiteUrl: 'https://myriad.example/',
    addSiteUrl: 'https://myriad.example/',
  })
  try {
    await ctx.setInputValue('Vision Pro')
    await ctx.submitAdd()

    const qrSvg = ctx.container.querySelector('svg[role="img"]')
    assert.ok(qrSvg, 'expected inline SVG QR code')
    assert.equal(
      qrSvg.getAttribute('aria-label'),
      '「Vision Pro」的绑定二维码',
    )
    const pathD = qrSvg.querySelector('path')?.getAttribute('d') ?? ''
    assert.ok(pathD.length > 50, 'expected QR modules path')

    const expectedUri = buildDeviceBindUri({
      site: 'https://myriad.example',
      token: ctx.token,
    })
    assert.equal(
      ctx.container.querySelector('[data-field="site"] dd')?.textContent,
      'https://myriad.example',
    )
    assert.equal(
      ctx.container.querySelector('[data-field="token"] dd')?.textContent,
      ctx.token,
    )
    assert.equal(
      ctx.container.querySelector('[data-field="bindUri"] dd')?.textContent,
      expectedUri,
    )
    assert.equal(
      ctx.container.querySelector('.agent-device-qr-warn'),
      null,
      'configured public siteUrl overrides localhost without loopback warning',
    )

    const doneBtn = ctx.container.querySelector(
      'button[data-action="done"]',
    ) as HTMLButtonElement
    assert.ok(doneBtn)
    await ctx.act(async () => {
      doneBtn.click()
    })
    assert.equal(ctx.container.querySelector('svg[role="img"]'), null)
    assert.equal(ctx.container.querySelector('[data-field="token"]'), null)
  }
  finally {
    await ctx.cleanup()
  }
})

test('shows loopback warning when no public siteUrl is configured on localhost', async () => {
  const ctx = await mountDevicesGroup({
    origin: 'http://localhost:1102',
    listSiteUrl: null,
    addSiteUrl: null,
  })
  try {
    await ctx.setInputValue('iPhone')
    await ctx.submitAdd()

    assert.ok(ctx.container.querySelector('svg[role="img"]'))
    assert.equal(
      ctx.container.querySelector('[data-field="site"] dd')?.textContent,
      'http://localhost:1102',
    )
    const warn = ctx.container.querySelector('.agent-device-qr-warn')
    assert.ok(warn, 'expected loopback warning')
    assert.equal(warn.textContent, zh.agentDevicesQrLoopbackWarn)
  }
  finally {
    await ctx.cleanup()
  }
})
