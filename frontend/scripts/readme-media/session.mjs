// Shared scene setup: `node scenes/<scene>.mjs <zh|ja|en> <light|dark>`.
import process from 'node:process'
import { LOCALES } from './content.mjs'
import { install, installRig } from './mocks.mjs'
import { BACKEND, open } from './render.mjs'

export const L = process.argv[2] || 'zh'
export const SCHEME = process.argv[3] || 'light'
export const TAG = `${L}-${SCHEME}`

/** Signs in as the instance owner (README_MEDIA_OWNER / README_MEDIA_PASSWORD) and returns the session token. */
export async function ownerToken() {
  const username = process.env.README_MEDIA_OWNER
  const password = process.env.README_MEDIA_PASSWORD
  if (!username || !password) throw new Error('README_MEDIA_OWNER / README_MEDIA_PASSWORD are not set')
  const r = await fetch(`${BACKEND}/api/auth/login`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ username, password }) })
  const cookie = r.headers.getSetCookie().map((c) => c.match(/^auth_token=([^;]+)/)).find(Boolean)
  if (!r.ok || !cookie) throw new Error(`sign-in failed: HTTP ${r.status}`)
  return cookie[1]
}

/** Owner-authenticated calls to the instance. */
export async function ownerApi() {
  const token = await ownerToken()
  return async (method, p, body) => {
    const r = await fetch(BACKEND + p, { method, headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined })
    const text = await r.text()
    if (!r.ok) throw new Error(`${method} ${p}: HTTP ${r.status} ${text.slice(0, 200)}`)
    return text ? JSON.parse(text) : null
  }
}

export async function start(opts = {}) {
  install(L, opts)
  installRig(L)
  const token = await ownerToken()
  const state = { cookies: [{ name: 'auth_token', value: token, domain: 'localhost', path: '/', expires: -1, httpOnly: true, secure: false, sameSite: 'Lax' }], origins: [] }
  return open({ state, locale: LOCALES[L], scheme: SCHEME, ...opts })
}
