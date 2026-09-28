import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

test('real pendant attachments move independently while roots remain on the host', async ({ page }, testInfo) => {
  test.setTimeout(120_000)
  const root = process.env.MEROPE_EARWEAR_ASSET
  test.skip(!root, 'Set MEROPE_EARWEAR_ASSET to a rig with hanging earrings')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
  await page.route('**/earwear-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/earwear-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/earwear-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
    const { writeAnime25DAttachmentTransform } = await import(`${modules}layerAttachment.ts`)
    const original = JSON.stringify(manifest)
    const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
    await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/earwear-atlas.png')
    player.resize(384, 512, 1)
    player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
    const earrings = player.layers.filter(layer => layer.earwearPhysics)
    const base = new Float32Array(9)
    let rootError = 0; let motion = 0; let determinant = 1; let disabledError = 0
    for (let i = 0; i < 240; i++) {
      const angle = Math.sin(i / 24) * 0.8
      const enabled = i < 210
      player.setTarget({ ...IDENTITY_DRIVER, idle: false, rand: false, blink: false, talk: false, mouse: false,
        phys: enabled, angleX: angle, angleY: angle * 0.6, angleZ: angle * 0.6 })
      player.tick(1 / 60)
      for (const layer of earrings) {
        const a = layer.attachment; const m = layer.layerTransform
        writeAnime25DAttachmentTransform(a, player.secondaryDeformationFrame, base)
        const px = t => t[0] * a.x + t[3] * a.y + t[6]
        const py = t => t[1] * a.x + t[4] * a.y + t[7]
        rootError = Math.max(rootError, Math.hypot(px(m) - px(base), py(m) - py(base)))
        determinant = Math.min(determinant, m[0] * m[4] - m[1] * m[3])
        if (enabled) motion = Math.max(motion, Math.abs(m[3] - base[3]) * layer.source.h)
        else disabledError = Math.max(disabledError, ...Array.from(m, (v, j) => Math.abs(v - base[j])))
      }
    }
    const error = player.gl.getError()
    player.dispose()
    return { names: earrings.map(layer => layer.source.name), rootError, motion, determinant, disabledError, error, unchanged: original === JSON.stringify(manifest) }
  }, { manifest, modules })
  await testInfo.attach('earwear-metrics', { body: JSON.stringify(result, null, 2), contentType: 'application/json' })
  expect(result.names.length).toBeGreaterThanOrEqual(2)
  expect(result.rootError).toBeLessThan(0.001)
  expect(result.motion).toBeGreaterThan(2)
  expect(result.determinant).toBeGreaterThan(0.75)
  expect(result.disabledError).toBe(0)
  expect(result.error).toBe(0)
  expect(result.unchanged).toBe(true)
})

test('real paired earrings attach independently without repacking their atlas', async ({ page }, testInfo) => {
  test.setTimeout(60_000)
  const root = process.env.MEROPE_COLLAR_ASSET
  test.skip(!root, 'Set MEROPE_COLLAR_ASSET to the paired-earring portrait')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
  await page.route('**/earwear-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/earwear-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/earwear-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
    const { bindAnime25DLayerAttachment, writeAnime25DAttachmentTransform } = await import(`${modules}layerAttachment.ts`)
    const { readLayerPixels, loadImage } = await import(`${modules}webglRuntime.ts`)
    const playback = manifest.anime25dPlayback
    const original = JSON.stringify(playback)
    const source = playback.layers.find(layer => layer.role === 'earwear' && !layer.side)
    if (!source) throw new Error('Fixture needs combined upstream earrings')
    const player = new Anime25DPlayer(document.querySelector('canvas'), playback, manifest)
    await player.replaceLivePackage(playback, manifest, '/earwear-atlas.png')
    player.resize(768, 1024, 1)
    player.shellActivation = 1
    const parts = player.layers.filter(layer => layer.source.role === 'earwear')
    if (parts.length !== 2 || parts.some(layer => !layer.attachment)) throw new Error('Both earrings need their own attachment')
    const image = await loadImage('/earwear-atlas.png')
    // The old shared attachment evaluated against the SAME final ear surface.
    const shared = bindAnime25DLayerAttachment(source, player.layers, playback.anchors, null, playback.pixelCanvas.width, layer => readLayerPixels(image, layer))
    if (!shared) throw new Error('The baseline shared attachment must bind')
    const matrix = new Float32Array(9)
    const screenshots = []
    const metrics = []
    for (const [angleX, angleY] of [[0, 0], [1, -0.85], [-1, 0.85]]) {
      Object.assign(player.current, IDENTITY_DRIVER, { angleX, angleY, idle: false, rand: false, blink: false, phys: false })
      player.time = 1
      player.deform(); player.uploadGeometry()
      writeAnime25DAttachmentTransform(shared, player.secondaryDeformationFrame, matrix)
      const retained = parts.map(layer => layer.layerTransform.slice())
      let beforeError = 0; let afterError = 0; let scaleError = 0
      for (const part of parts) {
        const a = part.attachment
        const transform = part.layerTransform
        const distance = m => Math.hypot(m[0] * a.x + m[3] * a.y + m[6] - a.origin.x, m[1] * a.x + m[4] * a.y + m[7] - a.origin.y)
        beforeError = Math.max(beforeError, distance(matrix))
        afterError = Math.max(afterError, distance(transform))
        scaleError = Math.max(scaleError, Math.abs(Math.hypot(transform[0], transform[1]) - 1))
        part.layerTransform.set(matrix)
      }
      player.draw()
      if (angleX === 1) screenshots.push({ name: 'shared-earrings', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
      parts.forEach((layer, i) => layer.layerTransform.set(retained[i]))
      player.draw()
      if (angleX === 1) screenshots.push({ name: 'independent-earrings', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
      metrics.push({ angleX, beforeError, afterError, scaleError })
    }
    const unchanged = original === JSON.stringify(playback)
    const error = player.gl.getError()
    player.dispose()
    return { unchanged, error, metrics, screenshots }
  }, { manifest, modules })
  await testInfo.attach('earwear-metrics', { body: JSON.stringify(result.metrics, null, 2), contentType: 'application/json' })
  for (const shot of result.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
  expect(result.unchanged).toBe(true)
  expect(result.error).toBe(0)
  for (const metric of result.metrics) {
    expect(metric.afterError).toBeLessThan(0.001)
    expect(metric.scaleError).toBeLessThan(1e-6)
    if (metric.angleX !== 0) expect(metric.beforeError).toBeGreaterThan(1)
    else expect(metric.beforeError).toBeLessThan(0.001)
  }
})
