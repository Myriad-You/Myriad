import type { AddressInfo } from 'node:net'
import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { once } from 'node:events'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { createServer as httpServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { backendDevProxyPlugin } from '../scripts/vite/backendDevProxy.mjs'
import { documentPlugin } from '../scripts/vite/documentPlugin'
import { siteBrandingStampPlugin } from '../scripts/vite/siteBrandingStampPlugin.mjs'
import { spaFallbackPlugin } from '../scripts/vite/spaFallback.mjs'

it('Vite serves deep links while backend routing precedes stamping and history fallback', async () => {
  const root = await mkdtemp(join(tmpdir(), 'myriad-vite-contract-'))
  let releaseStream = () => {}
  const mediaPath = '/media/assets/11111111-1111-4111-8111-111111111111/sticker.png'
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j6wAAAABJRU5ErkJggg==', 'base64')
  const backend = httpServer((req, res) => {
    if (req.url?.startsWith('/media/assets/')) {
      res.statusCode = req.url.startsWith(mediaPath) ? 200 : 404
      res.setHeader('Content-Type', 'image/png')
      res.end(req.method === 'HEAD' || res.statusCode === 404 ? undefined : png)
      return
    }
    if (req.url === '/api/config/metadata') {
      res.setHeader('Content-Type', 'application/json')
      res.end(JSON.stringify({ site_title: 'Fixture Brand', site_description: 'Fixture description' }))
    } else if (req.url === '/api/agent/process/stream') {
      res.setHeader('Content-Type', 'text/event-stream')
      res.write('data: first\n\n')
      releaseStream = () => res.end('data: last\n\n')
    } else {
      res.setHeader('Content-Type', 'text/html')
      res.end(`backend:${req.url}`)
    }
  })
  backend.listen(0, '127.0.0.1')
  await once(backend, 'listening')
  const backendTarget = `http://127.0.0.1:${(backend.address() as AddressInfo).port}`
  await writeFile(join(root, 'index.html'), await readFile(new URL('../index.html', import.meta.url), 'utf8'))
  const server = await createServer({
    configFile: false,
    root,
    appType: 'spa',
    logLevel: 'silent',
    server: { port: 0, host: '127.0.0.1' },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      documentPlugin(),
      siteBrandingStampPlugin({ backendTarget, frontendRoot: fileURLToPath(new URL('..', import.meta.url)) }),
      backendDevProxyPlugin({ backendTarget }),
      spaFallbackPlugin(),
    ],
  })
  try {
    await server.listen()
    const origin = `http://127.0.0.1:${(server.httpServer!.address() as AddressInfo).port}`
    for (const path of ['/', '/journal/articles/12?keep=1', '/tapp/run/abc', '/agent/settings']) {
      const response = await fetch(origin + path, { headers: { accept: 'text/html' } })
      assert.equal(response.status, 200)
      assert.match(response.headers.get('permissions-policy')!, /microphone=\(self\)/)
      const html = await response.text()
      assert.match(html, /Fixture Brand/)
      assert.match(html, /@font-face/)
      assert.match(html, /src="\/src\/main.tsx"/)
    }
    const manifest = await fetch(`${origin}/manifest.webmanifest`)
    assert.equal((await manifest.json()).name, 'Fixture Brand')
    for (const path of ['/api/example', '/robots.txt', '/sitemap.xml', '/journal/notes.xml', '/.well-known/webfinger']) {
      assert.equal(await (await fetch(origin + path)).text(), `backend:${path}`)
    }
    const media = await fetch(`${origin}${mediaPath}?version=1`)
    assert.equal(media.status, 200)
    assert.equal(media.headers.get('content-type'), 'image/png')
    assert.deepEqual(Buffer.from(await media.arrayBuffer()), png)
    const head = await fetch(origin + mediaPath, { method: 'HEAD' })
    assert.equal(head.status, 200)
    assert.equal((await head.arrayBuffer()).byteLength, 0)
    assert.equal((await fetch(`${origin}/media/assets/missing/photo.png`)).status, 404)
    const crawler = { accept: 'text/html', 'user-agent': 'Googlebot' }
    assert.equal(await (await fetch(`${origin}/journal/articles/12`, { headers: crawler })).text(), 'backend:/journal/articles/12')
    assert.match(await (await fetch(`${origin}/journal/articles/12?_spa=1`, { headers: crawler })).text(), /id="app-root"/)
    assert.equal((await fetch(`${origin}/assets/missing.js`, { headers: { accept: '*/*' } })).status, 404)
    const response = await fetch(`${origin}/api/agent/process/stream`)
    const reader = response.body!.getReader()
    const first = await reader.read()
    assert.match(new TextDecoder().decode(first.value), /data: first/)
    releaseStream()
    assert.match(new TextDecoder().decode((await reader.read()).value), /data: last/)
    await reader.cancel()
  } finally {
    releaseStream()
    await server.close()
    backend.closeAllConnections()
    await new Promise<void>((resolve) => backend.close(() => resolve()))
    await rm(root, { recursive: true, force: true })
  }
})
