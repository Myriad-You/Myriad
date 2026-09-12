import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

describe('home first-paint budget', () => {
  it('keeps Agora and Config off the home module graph', () => {
    const home = readFileSync(new URL('../views/Home.tsx', import.meta.url), 'utf8')
    const routes = readFileSync(
      new URL('./codeSplitting.ts', import.meta.url),
      'utf8',
    )
    const agora = readFileSync(
      new URL('../features/merope/speech/agoraConversation.ts', import.meta.url),
      'utf8',
    )
    assert.equal(home.includes('agora-rtc-sdk-ng'), false)
    assert.equal(home.includes('agora-rtm'), false)
    assert.equal(home.includes("from '../views/Config'"), false)
    assert.match(routes, /config:\s*lazyWithPreload\(\(\) => import\('\.\.\/views\/Config'\)\)/)
    assert.match(agora, /import\('agora-rtc-sdk-ng'\)/)
    assert.match(agora, /import\('agora-rtm'\)/)
  })

  it('counts Astro hydration entries and skips dynamic speech', async () => {
    const { measureHomeBudget } = await import('../../scripts/home-budget.mjs')
    const root = mkdtempSync(join(tmpdir(), 'home-budget-'))
    mkdirSync(join(root, 'assets'))
    writeFileSync(
      join(root, 'index.html'),
      '<astro-island component-url="/assets/App-test.js" renderer-url="/assets/client-test.js"></astro-island>',
    )
    writeFileSync(
      join(root, 'assets/App-test.js'),
      'import "./vendor-test.js"; import("./speech-test.js")',
    )
    writeFileSync(join(root, 'assets/client-test.js'), 'export default {}')
    writeFileSync(join(root, 'assets/vendor-test.js'), 'export default {}')
    writeFileSync(
      join(root, 'assets/speech-test.js'),
      'import("agora-rtc-sdk-ng")',
    )
    const measured = await measureHomeBudget(root)
    assert.ok(measured.files.some((file) => file.file.endsWith('App-test.js')))
    assert.ok(measured.files.some((file) => file.file.endsWith('client-test.js')))
    assert.ok(measured.files.some((file) => file.file.endsWith('vendor-test.js')))
    assert.equal(
      measured.files.some((file) => file.file.endsWith('speech-test.js')),
      false,
    )
    assert.equal(measured.loadsAgora, false)
    assert.equal(measured.loadsConfigRoute, false)
  })

  it('flags a statically imported Config chunk', async () => {
    const { measureHomeBudget } = await import('../../scripts/home-budget.mjs')
    const root = mkdtempSync(join(tmpdir(), 'home-budget-config-'))
    mkdirSync(join(root, 'assets'))
    writeFileSync(
      join(root, 'index.html'),
      '<astro-island component-url="/assets/App-test.js" renderer-url="/assets/client-test.js"></astro-island>',
    )
    writeFileSync(
      join(root, 'assets/App-test.js'),
      'import "./Config-test.js"',
    )
    writeFileSync(join(root, 'assets/client-test.js'), 'export default {}')
    writeFileSync(join(root, 'assets/Config-test.js'), 'export default {}')
    const measured = await measureHomeBudget(root)
    assert.equal(measured.loadsConfigRoute, true)
  })

  it('keeps built first-paint assets inside the gzip baseline when dist exists', async () => {
    const dist = new URL('../../dist/index.html', import.meta.url)
    if (!existsSync(fileURLToPath(dist))) {
      return
    }
    const { measureHomeBudget } = await import('../../scripts/home-budget.mjs')
    const measured = await measureHomeBudget()
    const baseline = JSON.parse(
      readFileSync(new URL('../../scripts/home-budget.baseline.json', import.meta.url), 'utf8'),
    ) as { jsGzipBytes: number; cssGzipBytes: number }
    assert.equal(measured.loadsAgora, false)
    assert.equal(measured.loadsConfigRoute, false)
    assert.ok(
      measured.files.some((file) => /(?:^|\/)App-[^/]+\.js$/.test(file.file)),
      `App hydration entry missing from first-paint: ${measured.files.map((f) => f.file).join(', ')}`,
    )
    assert.ok(
      measured.jsGzipBytes <= Math.ceil(baseline.jsGzipBytes * 1.15),
      `JS gzip ${measured.jsGzipBytes} > baseline ${baseline.jsGzipBytes} +15%`,
    )
    assert.ok(
      measured.cssGzipBytes <= Math.ceil(baseline.cssGzipBytes * 1.15),
      `CSS gzip ${measured.cssGzipBytes} > baseline ${baseline.cssGzipBytes} +15%`,
    )
  })
})
