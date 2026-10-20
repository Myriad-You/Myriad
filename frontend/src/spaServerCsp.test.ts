import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { CSP_NONCE_PLACEHOLDER } from '../scripts/vite/constants.mjs'

const server = fileURLToPath(new URL('../scripts/spa-server.mjs', import.meta.url))
const doc = `<!doctype html><html><head><meta property="csp-nonce" nonce="${CSP_NONCE_PLACEHOLDER}"><script nonce="${CSP_NONCE_PLACEHOLDER}">1</script></head></html>`

describe('spa-server document CSP', () => {
  const dist = mkdtempSync(path.join(tmpdir(), 'spa-csp-'))
  const port = 20000 + Math.floor(Math.random() * 20000)
  let child: ReturnType<typeof spawn>

  before(async () => {
    writeFileSync(path.join(dist, 'index.html'), doc)
    writeFileSync(path.join(dist, 'embed.html'), doc)
    child = spawn(process.execPath, [server], {
      env: { ...process.env, DIST_DIR: dist, PORT: String(port), HOST: '127.0.0.1', BRANDING_METADATA_URL: '' },
      stdio: ['ignore', 'pipe', 'inherit'],
    })
    await new Promise<void>((resolve) => child.stdout!.once('data', () => resolve()))
  })
  after(() => {
    child.kill()
    rmSync(dist, { recursive: true, force: true })
  })

  async function load(pathname: string) {
    const response = await fetch(`http://127.0.0.1:${port}${pathname}`, { headers: { 'accept-encoding': 'identity' } })
    const csp = response.headers.get('content-security-policy') || ''
    return { response, csp, body: await response.text(), nonce: /'nonce-([^']+)'/.exec(csp)?.[1] }
  }

  it('stamps a fresh nonce into the main document and forbids framing', async () => {
    const first = await load('/')
    const second = await load('/settings')
    assert.ok(first.nonce && second.nonce && first.nonce !== second.nonce)
    assert.ok(!first.body.includes(CSP_NONCE_PLACEHOLDER))
    assert.equal(first.body.split(`nonce="${first.nonce}"`).length, 3)
    assert.match(first.csp, /'strict-dynamic'/)
    assert.match(first.csp, /frame-ancestors 'none'/)
    assert.doesNotMatch(first.csp, /script-src[^;]*'unsafe-/)
    assert.equal(first.response.headers.get('x-frame-options'), 'DENY')
  })

  it('lets only this origin frame the embed document', async () => {
    const embed = await load('/embed/comments')
    assert.match(embed.csp, /frame-ancestors 'self'/)
    assert.equal(embed.response.headers.get('x-frame-options'), 'SAMEORIGIN')
    assert.ok(embed.body.includes(`nonce="${embed.nonce}"`))
  })
})
