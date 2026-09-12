import type { APIRequestContext, Playwright } from '@playwright/test'
/**
 * API integration smoke against a real backend + disposable Postgres.
 * Not a production-page UI end-to-end suite.
 *
 * Each chain fails if the write is skipped, a private gate is relaxed, or
 * logout / grant rebind is not enforced.
 */
import {

  expect,

  test,
} from '@playwright/test'

const USER = 'smokeadmin'
const PASS = 'SmokePass1'
const SETUP_SECRET = process.env.MYRIAD_SETUP_SECRET || 'smoke-setup-secret'
const TAPP_ID = 'com.myriad.smoke-hello'
const BASE_URL = process.env.MYRIAD_SMOKE_BASE_URL || 'http://127.0.0.1:18103'
const ORIGIN = 'http://localhost:1102'

const MINIMAL_CORE = 'exports.onReady = function () {}'

function coreManifest(permissions: string[]) {
  return {
    id: TAPP_ID,
    name: 'Smoke Hello',
    version: '1.0.0',
    minSystemVersion: '0.4.0',
    category: 'utility',
    core: { entry: 'core.js' },
    permissions,
  }
}

function installBody(permissions: string[]) {
  return {
    source: 'direct',
    manifest: coreManifest(permissions),
    modules: { 'core.js': MINIMAL_CORE },
    permissions,
  }
}

async function openContext(
  playwright: Playwright,
  storageState?: Awaited<ReturnType<APIRequestContext['storageState']>>,
) {
  return playwright.request.newContext({
    baseURL: BASE_URL,
    extraHTTPHeaders: { Origin: ORIGIN },
    storageState,
  })
}

async function csrf(request: APIRequestContext): Promise<string> {
  const response = await request.get('/api/csrf-token')
  expect(response.ok(), await response.text()).toBeTruthy()
  const body = (await response.json()) as { csrf_token?: string | null }
  expect(body.csrf_token, 'logged-in session must receive a CSRF token').toBeTruthy()
  return body.csrf_token as string
}

async function storageUsage(request: APIRequestContext, grant: string) {
  return request.get(`/api/tapps/${TAPP_ID}/storage/usage`, {
    headers: { 'X-Tapp-Runtime-Grant': grant },
  })
}

test.describe.serial('business API smoke', () => {
  test('init, login, save settings, refresh reads them back', async ({
    request,
    playwright,
  }) => {
    const setup = await request.post('/api/setup/create-admin', {
      data: {
        username: USER,
        password: PASS,
        setup_secret: SETUP_SECRET,
      },
    })
    const setupStatus = setup.status()
    expect(
      setupStatus === 200 || setupStatus === 409,
      `create-admin ${setupStatus} ${await setup.text()}`,
    ).toBeTruthy()

    const login = await request.post('/api/auth/login', {
      data: { username: USER, password: PASS },
    })
    expect(login.ok(), await login.text()).toBeTruthy()

    const token = await csrf(request)
    const saved = await request.put('/api/config/module-visibility', {
      headers: { 'X-CSRF-Token': token },
      data: {
        modules: {
          library: 'all',
          brew: 'authenticated',
          reports: 'all',
          tapp: 'all',
          agent: 'all',
        },
        agentUsage: { guest: 'none', user: 'standard' },
      },
    })
    expect(saved.ok(), await saved.text()).toBeTruthy()

    const fresh = await openContext(playwright)
    try {
      const loginAgain = await fresh.post('/api/auth/login', {
        data: { username: USER, password: PASS },
      })
      expect(loginAgain.ok(), await loginAgain.text()).toBeTruthy()
      const read = await fresh.get('/api/config/module-visibility')
      expect(read.ok(), await read.text()).toBeTruthy()
      const body = (await read.json()) as {
        preferences?: { modules?: { brew?: string } }
      }
      expect(
        body.preferences?.modules?.brew,
        'skipping the PUT would leave brew at the default "all"',
      ).toBe('authenticated')
    } finally {
      await fresh.dispose()
    }
  })

  test('public guest can read public config; private tapp is hidden', async ({
    request,
    playwright,
  }) => {
    const login = await request.post('/api/auth/login', {
      data: { username: USER, password: PASS },
    })
    expect(login.ok(), await login.text()).toBeTruthy()
    const token = await csrf(request)

    const installed = await request.post('/api/tapps/install', {
      headers: { 'X-CSRF-Token': token },
      data: installBody(['storage:read', 'ui:theme']),
    })
    const installedStatus = installed.status()
    expect(
      installedStatus === 200 || installedStatus === 409,
      await installed.text(),
    ).toBeTruthy()

    const guest = await openContext(playwright)
    try {
      const pub = await guest.get('/api/config/public')
      expect(pub.status(), 'public config must stay guest-readable').toBe(200)

      const adminConfig = await guest.get('/api/config')
      expect(
        adminConfig.status(),
        'relaxing this to 200 would expose admin config',
      ).toBe(401)

      const visible = await guest.get(`/api/tapps/${TAPP_ID}`)
      expect(visible.ok(), await visible.text()).toBeTruthy()

      const hide = await request.post(`/api/tapps/${TAPP_ID}/visibility`, {
        headers: { 'X-CSRF-Token': token },
        data: { visibility: 'admin' },
      })
      expect(hide.ok(), await hide.text()).toBeTruthy()

      const hidden = await guest.get(`/api/tapps/${TAPP_ID}`)
      expect(
        hidden.status(),
        'skipping visibility=admin would keep this 200 for guests',
      ).toBe(404)
    } finally {
      await guest.dispose()
    }
  })

  test('tapp grant change and logout kill the previous runtime', async ({
    request,
    playwright,
  }) => {
    const login = await request.post('/api/auth/login', {
      data: { username: USER, password: PASS },
    })
    expect(login.ok(), await login.text()).toBeTruthy()
    const token = await csrf(request)

    const installed = await request.post('/api/tapps/install', {
      headers: { 'X-CSRF-Token': token },
      data: installBody(['storage:read', 'ui:theme']),
    })
    const installedStatus = installed.status()
    expect(
      installedStatus === 200 || installedStatus === 409,
      await installed.text(),
    ).toBeTruthy()

    const reveal = await request.post(`/api/tapps/${TAPP_ID}/visibility`, {
      headers: { 'X-CSRF-Token': token },
      data: { visibility: 'all' },
    })
    expect(reveal.ok(), await reveal.text()).toBeTruthy()

    const wide = await request.post(`/api/tapps/${TAPP_ID}/update`, {
      headers: { 'X-CSRF-Token': token },
      data: installBody(['storage:read', 'ui:theme']),
    })
    expect(wide.ok(), await wide.text()).toBeTruthy()

    const oldGrantRes = await request.post(`/api/tapps/${TAPP_ID}/runtime-grants`, {
      headers: { 'X-CSRF-Token': token },
      data: { instanceId: 'smoke-page-old', kind: 'page' },
    })
    expect(oldGrantRes.ok(), await oldGrantRes.text()).toBeTruthy()
    const oldGrant = (await oldGrantRes.json()) as {
      token?: string
      permissions?: string[]
    }
    expect(oldGrant.token).toBeTruthy()
    expect(oldGrant.permissions ?? []).toContain('storage:read')

    const oldAllowed = await storageUsage(request, oldGrant.token ?? '')
    expect(
      oldAllowed.ok(),
      `old grant must work on a RuntimeGrantContext route: ${await oldAllowed.text()}`,
    ).toBeTruthy()

    const shrunk = await request.post(`/api/tapps/${TAPP_ID}/update`, {
      headers: { 'X-CSRF-Token': token },
      data: installBody(['ui:theme']),
    })
    expect(shrunk.ok(), await shrunk.text()).toBeTruthy()

    const oldDenied = await storageUsage(request, oldGrant.token ?? '')
    expect(
      oldDenied.status(),
      'install update revokes every prior grant; the old token is Invalid (401), not 403',
    ).toBe(401)

    const newGrantRes = await request.post(`/api/tapps/${TAPP_ID}/runtime-grants`, {
      headers: { 'X-CSRF-Token': token },
      data: { instanceId: 'smoke-page-new', kind: 'page' },
    })
    expect(newGrantRes.ok(), await newGrantRes.text()).toBeTruthy()
    const newGrant = (await newGrantRes.json()) as {
      token?: string
      permissions?: string[]
    }
    expect(newGrant.token).toBeTruthy()
    expect(
      newGrant.permissions ?? [],
      'approved-permission shrink must drop storage:read from the new grant',
    ).not.toContain('storage:read')
    expect(newGrant.permissions ?? []).toContain('ui:theme')

    const newDenied = await storageUsage(request, newGrant.token ?? '')
    expect(newDenied.status(), 'new grant must not keep storage:read').toBe(403)

    const staleState = await request.storageState()
    const stale = await openContext(playwright, staleState)
    try {
      const logout = await request.post('/api/auth/logout')
      expect(logout.ok(), await logout.text()).toBeTruthy()

      const after = await stale.get('/api/auth/me')
      expect(after.ok()).toBeTruthy()
      const guestMe = (await after.json()) as { authenticated?: boolean }
      expect(
        guestMe.authenticated,
        'copied cookies after logout must not stay authenticated',
      ).toBe(false)

      const staleGrant = await storageUsage(stale, oldGrant.token ?? '')
      expect(
        staleGrant.status(),
        'an old cookie+grant must not pass RuntimeGrantContext after logout',
      ).toBe(401)
    } finally {
      await stale.dispose()
    }
  })
})
