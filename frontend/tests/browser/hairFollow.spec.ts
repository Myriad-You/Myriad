import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { delimiter } from 'node:path'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const assets = [
  ['off-shoulder', process.env.MEROPE_SHOULDER_ASSET],
  ['high-collar', process.env.MEROPE_COLLAR_ASSET],
  ...(process.env.MEROPE_HAIR_ASSETS ?? '').split(delimiter).filter(Boolean).map((root, i) => [`additional-${i + 1}`, root]),
]

test('real high-collar crown replay changes scalp pixels without repainting the silhouette or lower features', async ({ page }, testInfo) => {
  test.setTimeout(60_000)
  const root = process.env.MEROPE_COLLAR_ASSET
  test.skip(!root, 'Set MEROPE_COLLAR_ASSET to the high-collar split portrait')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
  await page.route('**/crown-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/crown-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/crown-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
    const original = JSON.stringify(manifest)
    const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
    await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/crown-atlas.png')
    player.resize(768, 1024, 1)
    player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
    const face = player.layers.find(layer => layer.source.role === 'face')
    if (!face?.crownOccluders?.length) throw new Error('Fixture must exercise the evidence-gated cap, not pass with a disabled feature')
    const crowns = face.crownOccluders
    const gl = player.gl
    const capture = () => {
      player.draw()
      const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4)
      gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
      return pixels
    }
    const screenshots = []; const poses = []
    for (const [angleX, angleY] of [[0, 0], [0.65, 0.55], [-0.65, -0.55], [0, -0.7]]) {
      Object.assign(player.current, IDENTITY_DRIVER, { angleX, angleY, idle: false, rand: false, blink: false, talk: false, phys: false })
      player.deform(); player.uploadGeometry()
      const after = capture()
      screenshots.push({ name: `cap-${angleX}-${angleY}`, image: gl.canvas.toDataURL('image/png').split(',')[1] })
      face.crownOccluders = undefined
      const before = capture()
      screenshots.push({ name: `without-cap-${angleX}-${angleY}`, image: gl.canvas.toDataURL('image/png').split(',')[1] })
      const visible = player.layers.map(layer => [layer.frameOpacity, layer.retainWhenHidden])
      player.layers.forEach(layer => { layer.frameOpacity = layer === face ? 1 : 0; layer.retainWhenHidden = false })
      const faceMask = capture()
      const protectedRoles = new Set(['iris', 'eyewhite', 'mouth', 'neck', 'collar-front', 'collar-back', 'topwear'])
      player.layers.forEach((layer, i) => {
        layer.frameOpacity = protectedRoles.has(layer.source.role) ? visible[i][0] : 0
        layer.retainWhenHidden = protectedRoles.has(layer.source.role) && visible[i][1]
      })
      const protectedMask = capture()
      player.layers.forEach((layer, i) => { [layer.frameOpacity, layer.retainWhenHidden] = visible[i] })
      face.crownOccluders = crowns
      let changed = 0; let outsideFace = 0; let changedProtected = 0; let protectedPixels = 0
      for (let i = 0; i < before.length; i += 4) {
        if (protectedMask[i + 3] >= 254) protectedPixels++
        if (Math.max(...after.slice(i, i + 4).map((v, c) => Math.abs(v - before[i + c]))) <= 2) continue
        changed++
        if (faceMask[i + 3] === 0) outsideFace++
        if (protectedMask[i + 3] >= 254) changedProtected++
      }
      poses.push({ angleX, angleY, changed, outsideFace, changedProtected, protectedPixels })
    }
    const error = gl.getError()
    player.dispose()
    return { poses, screenshots, error, unchanged: original === JSON.stringify(manifest) }
  }, { manifest, modules })
  await testInfo.attach('crown-metrics', { body: JSON.stringify({ ...result, screenshots: undefined }, null, 2), contentType: 'application/json' })
  for (const shot of result.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
  expect(result.unchanged).toBe(true)
  expect(result.error).toBe(0)
  for (const pose of result.poses) {
    expect(pose.changed).toBeGreaterThan(100)
    expect(pose.outsideFace).toBe(0)
    expect(pose.changedProtected).toBe(0)
    expect(pose.protectedPixels).toBeGreaterThan(1000)
  }
})

test('real music-to-idle transition keeps the whole player moving without resetting the head or hair', async ({ page }, testInfo) => {
  test.setTimeout(180_000)
  const root = process.env.MEROPE_COLLAR_ASSET
  test.skip(!root, 'Set MEROPE_COLLAR_ASSET to a real rig')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
  await page.route('**/music-transition-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/music-transition-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/music-transition-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
    const { musicSignalAt } = await import(`${modules}../singing/musicSignal.test-support.ts`)
    const random = Math.random
    const runs = []
    try {
      for (const fps of [30, 60, 120]) {
        let seed = 1749
        Math.random = () => ((seed = Math.imul(seed, 1664525) + 1013904223 >>> 0) / 4294967296)
        const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
        await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/music-transition-atlas.png')
        player.resize(384, 512, 1)
        player.setTarget({ ...IDENTITY_DRIVER, talk: false, blink: false })
        player.setMotionPolicy({ mouth: 'idle', expression: 'idle', gaze: 'idle', headBody: 'music' })
        player.setSingingTrack('transition-regression')
        player.setSinging(true)
        const face = player.layers.find(layer => layer.source.role === 'face')
        const ranges = { music: [Infinity, -Infinity], idle: [Infinity, -Infinity] }
        const screenshots = []
        let previous = null; let velocity = null
        let stopPositionReset = 0; let peakTransitionAcceleration = 0; let idleHairTravel = 0
        let previousHair = null; let musicContribution = 0
        for (let frame = 0; frame < fps * 14; frame++) {
          const time = frame / fps
          if (frame === fps * 6) {
            const before = JSON.stringify(player.getCurrent())
            player.setSinging(false); player.setMusicSignal(null); player.clearBehaviorMotionUnits()
            player.setMotionPolicy({ mouth: 'idle', expression: 'idle', gaze: 'idle', headBody: 'idle' })
            stopPositionReset = Number(JSON.stringify(player.getCurrent()) !== before)
          }
          if (time < 6) player.setMusicSignal(musicSignalAt(time, { bpm: 100 }))
          player.tick(1 / fps)
          const pose = player.getCurrent()
          if (time >= 3 && time < 6) musicContribution = Math.max(musicContribution, Math.abs(player.singingGroove.output.angleZ * player.poseGate.output.groove.headBody))
          const range = time >= 9 ? ranges.idle : time >= 3 && time < 6 ? ranges.music : null
          if (range) { range[0] = Math.min(range[0], pose.angleX); range[1] = Math.max(range[1], pose.angleX) }
          const vertices = face.deformed
          if (previous) {
            const nextVelocity = Float32Array.from(vertices, (v, i) => (v - previous[i]) * fps)
            if (velocity && time >= 5.8 && time <= 6.8) {
              for (let i = 0; i < nextVelocity.length; i++) peakTransitionAcceleration = Math.max(peakTransitionAcceleration, Math.abs(nextVelocity[i] - velocity[i]) * fps)
            }
            velocity = nextVelocity
          }
          previous = vertices.slice()
          const hair = player.layers.filter(layer => layer.hairSurface).map(layer => layer.deformed.slice())
          if (previousHair && time >= 9) { hair.forEach((positions, k) => {
            for (let i = 0; i < positions.length; i++) idleHairTravel = Math.max(idleHairTravel, Math.abs(positions[i] - previousHair[k][i]) * fps)
          }) }
          previousHair = hair
          if (fps === 60 && [5, 6, 7, 10, 13].includes(time)) screenshots.push({ name: `music-exit-${time}s`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
        }
        runs.push({ fps, stopPositionReset, peakTransitionAcceleration, musicYawRange: ranges.music[1] - ranges.music[0],
          idleYawRange: ranges.idle[1] - ranges.idle[0], idleHairTravel, musicContribution, error: player.gl.getError(), screenshots })
        player.dispose()
      }
    } finally { Math.random = random }
    return runs
  }, { manifest, modules })
  await testInfo.attach('music-transition-metrics', { body: JSON.stringify(result.map(({ screenshots: _shots, ...run }) => run), null, 2), contentType: 'application/json' })
  for (const run of result) {
    for (const shot of run.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
    expect(run.stopPositionReset).toBe(0)
    expect(run.musicYawRange).toBeGreaterThan(0.1)
    expect(run.musicContribution).toBeGreaterThan(0.1)
    expect(run.idleYawRange).toBeGreaterThan(0.1)
    expect(run.idleHairTravel).toBeGreaterThan(5)
    expect(run.peakTransitionAcceleration).toBeLessThan(18000)
    expect(run.error).toBe(0)
  }
})

test('real thinking-to-speech handoff preserves a newer face and releases the old pose through the player', async ({ page }, testInfo) => {
  test.setTimeout(180_000)
  const root = process.env.MEROPE_COLLAR_ASSET
  test.skip(!root, 'Set MEROPE_COLLAR_ASSET to a real rig')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
  await page.route('**/thought-transition-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/thought-transition-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/thought-transition-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
    const { THINKING_EXPRESSION_PRESET } = await import(`${modules}expressionPresets.ts`)
    const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
    await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/thought-transition-atlas.png')
    player.resize(384, 512, 1)
    player.replaceTarget({ ...IDENTITY_DRIVER, ...THINKING_EXPRESSION_PRESET, talk: false, idle: false, rand: false, blink: false })
    let reset = false; let preservedBrow = 0; let retainedThinking = true
    let previous = player.getCurrent(); let maxHeadStep = 0; let mouthPeak = 0
    const screenshots = []
    for (let frame = 0; frame < 300; frame++) {
      if (frame === 90) {
        // An independent newer face writes the SAME value as the thought
        // preset. A later speech event must not mistake it for an old value.
        player.setTarget({ brow: 0.24 })
        const before = JSON.stringify(player.getCurrent())
        player.setSpeechActive(true)
        player.setTarget({ talk: true })
        player.setMotionPolicy({ mouth: 'speech', expression: 'idle', gaze: 'idle', headBody: 'coSpeech' })
        player.enqueueSpeechText('我想好了。这个办法可以试一试，我们慢慢来。', 'zh-CN')
        reset = before !== JSON.stringify(player.getCurrent())
        preservedBrow = player.getTarget().brow
        retainedThinking = player.getTarget().thinking
      }
      if (frame === 210) {
        player.clearSpeechText(); player.setSpeechActive(false)
        player.setTarget({ talk: false, mouthOpen: 0 })
        player.setMotionPolicy({ mouth: 'idle', expression: 'idle', gaze: 'idle', headBody: 'idle' })
      }
      player.tick(1 / 60)
      const current = player.getCurrent()
      if (frame >= 89 && frame < 150) {
        for (const key of ['angleX', 'angleY', 'angleZ', 'body']) {
          maxHeadStep = Math.max(maxHeadStep, Math.abs(current[key] - previous[key]))
        }
      }
      if (frame >= 100 && frame < 210) mouthPeak = Math.max(mouthPeak, current.mouthOpen)
      previous = current
      if ([60, 120, 240].includes(frame)) screenshots.push({ name: `thinking-speech-${frame}`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
    }
    const thoughtResidual = Math.max(...Object.values(player.thinkingMotion.output).map(Number).map(Math.abs))
    const error = player.gl.getError()
    player.dispose()
    return { reset, preservedBrow, retainedThinking, maxHeadStep, mouthPeak, thoughtResidual, error, screenshots }
  }, { manifest, modules })
  await testInfo.attach('thinking-speech-metrics', { body: JSON.stringify({ ...result, screenshots: undefined }, null, 2), contentType: 'application/json' })
  for (const shot of result.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
  expect(result.reset).toBe(false)
  expect(result.preservedBrow).toBe(0.24)
  expect(result.retainedThinking).toBe(false)
  expect(result.maxHeadStep).toBeLessThan(0.2)
  expect(result.mouthPeak).toBeGreaterThan(0.15)
  expect(result.thoughtResidual).toBeLessThan(0.001)
  expect(result.error).toBe(0)
})

for (const [name, root] of assets) {
  test(`${name}: shared canvas cut stays joined without changing upper motion`, async ({ page }, testInfo) => {
    test.setTimeout(120_000)
    test.skip(!root, 'Set MEROPE_HAIR_ASSETS to include cropped portraits')
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/cut-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/cut-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/cut-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
      await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/cut-atlas.png')
      player.resize(768, 1024, 1)
      player.shellActivation = 1
      player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
      const bound = player.layers.filter(layer => layer.cropBoundary)
      const bindings = bound.map(layer => layer.cropBoundary)
      let maxError = 0; let upperChanges = 0; let newFolds = 0; let baselineError = 0
      const shots = []
      const area = (vertices, indices, i) => {
        const a = indices[i] * 2; const b = indices[i + 1] * 2; const c = indices[i + 2] * 2
        return (vertices[b] - vertices[a]) * (vertices[c + 1] - vertices[a + 1]) - (vertices[b + 1] - vertices[a + 1]) * (vertices[c] - vertices[a])
      }
      for (const sign of [0, 1, -1, 0.6, -0.6]) {
        player.setTarget({ ...IDENTITY_DRIVER, idle: false, rand: false, blink: false, talk: false, mouse: false, phys: true, angleX: sign, angleY: sign * 0.7, angleZ: sign * 0.6, body: sign, armY: sign, armPos: sign * 0.5 })
        for (let frame = 0; frame < 90; frame++) player.tick(1 / 60)
        for (const layer of bound) layer.cropBoundary = undefined
        player.deform()
        const before = bound.map(layer => layer.deformed.slice())
        bound.forEach((layer, i) => { layer.cropBoundary = bindings[i] })
        player.deform(); player.uploadGeometry(); player.draw()
        for (let n = 0; n < bound.length; n++) {
          const layer = bound[n]; const binding = bindings[n]
          const host = binding.host.deformed; const edge = binding.edge
          for (const i of binding.ownEdge) {
            const x = layer.deformed[i]
            let j = 1
            while (j < edge.length - 1 && host[edge[j]] < x) j++
            const a = edge[j - 1]; const b = edge[j]
            const expected = host[a + 1] + (x - host[a]) / (host[b] - host[a]) * (host[b + 1] - host[a + 1])
            maxError = Math.max(maxError, Math.abs(layer.deformed[i + 1] - expected))
            baselineError = Math.max(baselineError, Math.abs(before[n][i + 1] - expected))
          }
          for (let i = 0; i < binding.weights.length; i++) {
            if (!binding.weights[i] && (before[n][i * 2] !== layer.deformed[i * 2] || before[n][i * 2 + 1] !== layer.deformed[i * 2 + 1])) upperChanges++
          }
          for (let i = 0; i < layer.indices.length; i += 3) {
            if (area(before[n], layer.indices, i) * area(layer.deformed, layer.indices, i) < 0) newFolds++
          }
        }
        shots.push({ name: `cut-${sign}`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
      }
      const error = player.gl.getError()
      player.dispose()
      return { names: bound.map(layer => layer.source.name), maxError, baselineError, upperChanges, newFolds, error, shots }
    }, { manifest, modules })
    await testInfo.attach('cut-metrics', { body: JSON.stringify({ ...result, shots: undefined }), contentType: 'application/json' })
    for (const shot of result.shots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
    expect(result.error).toBe(0)
    // Natural silhouettes are deliberately unbound; they cannot validate a cut.
    test.skip(!result.names.length, 'This asset has no shared opaque canvas cut')
    expect(result.names.length).toBeGreaterThan(0)
    expect(result.baselineError).toBeGreaterThan(1)
    expect(result.maxError).toBeLessThan(0.001)
    expect(result.upperChanges).toBe(0)
    expect(result.newFolds).toBe(0)
    expect(result.error).toBe(0)
  })
  test(`real ${name} visible head surfaces remain oriented at combined pose corners`, async ({ page }, testInfo) => {
    test.setTimeout(120_000)
    test.skip(!root, 'Provide a real split portrait')
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/pose-corners-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/pose-corners-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/pose-corners-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const { deriveAnime25DMotionEnvelopeProfile, projectAnime25DMotionEnvelope } = await import(`${modules}motionEnvelope.ts`)
      const original = JSON.stringify(manifest)
      const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
      await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/pose-corners-atlas.png')
      player.resize(768, 1024, 1)
      player.shellActivation = 1
      const bitmap = new Image()
      bitmap.src = '/pose-corners-atlas.png'
      await bitmap.decode()
      const flat = document.createElement('canvas')
      flat.width = bitmap.width; flat.height = bitmap.height
      const ctx = flat.getContext('2d')!
      ctx.drawImage(bitmap, 0, 0)
      const pixels = ctx.getImageData(0, 0, flat.width, flat.height).data
      const area = (p, a, b, c) => (p[b] - p[a]) * (p[c + 1] - p[a + 1]) - (p[b + 1] - p[a + 1]) * (p[c] - p[a])
      const surfaces = player.layers.filter(layer => ['face', 'front-hair', 'back-hair'].includes(layer.source.role)).map(layer => {
        const triangles = []
        const source = layer.source
        const alpha = (x, y) => {
          const ax = Math.min(flat.width - 1, Math.max(0, Math.floor((source.atlas.x + (x - source.x) / source.w * source.atlas.w) * flat.width)))
          const ay = Math.min(flat.height - 1, Math.max(0, Math.floor((source.atlas.y + (y - source.y) / source.h * source.atlas.h) * flat.height)))
          return pixels[(ay * flat.width + ax) * 4 + 3]
        }
        for (let i = 0; i < layer.indices.length; i += 3) {
          const indices = Array.from(layer.indices.slice(i, i + 3), index => Number(index) * 2)
          const [a, b, c] = indices
          const r = layer.rest
          const reference = area(r, a, b, c)
          if (Math.abs(reference) < 1e-8) continue
          // Ignore only genuinely transparent triangles, not a whole layer's bounds.
          const coverage = Math.max(alpha(r[a], r[a + 1]), alpha(r[b], r[b + 1]), alpha(r[c], r[c + 1]),
            alpha((r[a] + r[b] + r[c]) / 3, (r[a + 1] + r[b + 1] + r[c + 1]) / 3))
          if (coverage >= 128) triangles.push({ indices, reference })
        }
        return { layer, triangles }
      })
      const envelope = deriveAnime25DMotionEnvelopeProfile(manifest.anime25dPlayback, manifest)
      const screenshots = []; const runs = []
      const face = player.layers.find(layer => layer.source.role === 'face')
      Object.assign(player.current, IDENTITY_DRIVER, { idle: false, rand: false, blink: false, talk: false, phys: false })
      player.deform(); player.uploadGeometry(); player.draw()
      screenshots.push({ pose: 'neutral', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
      const occluders = face.crownOccluders
      face.crownOccluders = undefined
      player.draw()
      screenshots.push({ pose: 'neutral-without-replay', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
      face.crownOccluders = occluders
      for (const angleX of [-1, 1]) { for (const angleY of [-1, 1]) { for (const angleZ of [-0.8, 0.8]) {
        const driver = { ...IDENTITY_DRIVER, angleX, angleY, angleZ, body: angleZ,
          idle: false, rand: false, blink: false, talk: false, phys: false }
        projectAnime25DMotionEnvelope(driver, envelope, { clippedEnergy: 0, transferredEnergy: 0 })
        Object.assign(player.current, driver)
        player.deform(); player.uploadGeometry(); player.draw()
        let minimum = Infinity; let worst = null; let visibleTriangles = 0
        for (const { layer, triangles } of surfaces) { for (const { indices, reference } of triangles) {
          const [a, b, c] = indices
          const ratio = area(layer.deformed, a, b, c) / reference
          visibleTriangles++
          if (ratio < minimum) { minimum = ratio; worst = { layer: layer.source.name, role: layer.source.role, indices } }
        }
        }
        const pose = `${angleX}:${angleY}:${angleZ}`
        runs.push({ pose, minimum, worst, visibleTriangles })
        screenshots.push({ pose, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
      }
      } }
      const error = player.gl.getError()
      player.dispose()
      return { runs, screenshots, error, unchanged: original === JSON.stringify(manifest) }
    }, { manifest, modules })
    await testInfo.attach('combined-corner-metrics', { body: JSON.stringify(result.runs, null, 2), contentType: 'application/json' })
    for (const shot of result.screenshots) await testInfo.attach(`corner-${shot.pose}.png`, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
    expect(result.error).toBe(0)
    expect(result.unchanged).toBe(true)
    for (const run of result.runs) {
      expect(run.visibleTriangles).toBeGreaterThan(100)
      expect(run.minimum, JSON.stringify(run)).toBeGreaterThan(0)
    }
  })

  test(`real ${name} nod drives signed vertical hair lag into rendered pixels`, async ({ page }, testInfo) => {
    test.setTimeout(60_000)
    test.skip(!root, 'Set MEROPE_SHOULDER_ASSET / MEROPE_COLLAR_ASSET or MEROPE_HAIR_ASSETS to real split portraits')
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/hair-nod-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/hair-nod-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/hair-nod-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
      await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/hair-nod-atlas.png')
      player.resize(768, 1024, 1)
      player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
      Object.assign(player.current, IDENTITY_DRIVER, { talk: false })
      const springs = player.layers.flatMap(layer => layer.springs ?? [])
      if (!springs.length) throw new Error('Fixture needs hair springs')
      const area = (vertices, a, b, c) => (vertices[b] - vertices[a]) * (vertices[c + 1] - vertices[a + 1]) -
        (vertices[b + 1] - vertices[a + 1]) * (vertices[c] - vertices[a])
      const screenshots = []
      let minLag = 0; let maxLag = 0; let minAreaRatio = Infinity
      let visibleLag = 0; let changedPixels = 0
      for (let frame = 0; frame < 180; frame++) {
        const angleY = frame < 30 ? 0.85 : frame < 60 ? -0.85 : 0
        player.setTarget({ ...IDENTITY_DRIVER, angleY, idle: false, rand: false, blink: false, talk: false, phys: true })
        player.time += 1 / 60
        player.smoothDriver(1 / 60); player.updateSprings(1 / 60); player.deform()
        minLag = Math.min(minLag, ...springs.map(spring => spring.vertical.dx))
        maxLag = Math.max(maxLag, ...springs.map(spring => spring.vertical.dx))
        for (const layer of player.layers) {
          if (!layer.hairSurface) continue
          for (let i = 0; i < layer.indices.length; i += 3) {
            const [a, b, c] = Array.from(layer.indices.slice(i, i + 3), v => Number(v) * 2)
            const restArea = area(layer.rest, a, b, c)
            if (Math.abs(restArea) > 1e-8) minAreaRatio = Math.min(minAreaRatio, area(layer.deformed, a, b, c) / restArea)
          }
        }
        if (frame !== 14 && frame !== 44) continue
        const following = player.layers.map(layer => new Float32Array(layer.deformed))
        const gl = player.gl
        const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4)
        const capture = (name) => {
          player.uploadGeometry(); player.draw()
          screenshots.push({ name, image: gl.canvas.toDataURL('image/png').split(',')[1] })
          gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
          return pixels.slice()
        }
        const after = capture(`vertical-follow-${frame}`)
        // Isolate the new vertical channel at the SAME pose and spring time.
        // This is an ablation, not a recreation of all historical rendering.
        const lag = springs.map(spring => spring.vertical.dx)
        springs.forEach(spring => { spring.vertical.dx = 0 })
        player.deform()
        const before = capture(`without-vertical-follow-${frame}`)
        for (let i = 0; i < after.length; i += 4) {
          if (Math.max(...after.slice(i, i + 4).map((value, k) => Math.abs(value - before[i + k]))) > 8) changedPixels++
        }
        player.layers.forEach((layer, index) => {
          if (!layer.hairSurface) return
          for (let i = 1; i < layer.deformed.length; i += 2) visibleLag = Math.max(visibleLag, Math.abs(following[index][i] - layer.deformed[i]))
        })
        springs.forEach((spring, index) => { spring.vertical.dx = lag[index] })
      }
      const error = player.gl.getError()
      player.dispose()
      return { minLag, maxLag, minAreaRatio, visibleLag, changedPixels, error, screenshots }
    }, { manifest, modules })
    await testInfo.attach('hair-nod-metrics', { body: JSON.stringify({ ...result, screenshots: undefined }, null, 2), contentType: 'application/json' })
    for (const shot of result.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
    expect(result.minLag).toBeLessThan(-1)
    expect(result.maxLag).toBeGreaterThan(1)
    expect(result.visibleLag).toBeGreaterThan(1)
    expect(result.changedPixels).toBeGreaterThan(100)
    expect(result.minAreaRatio).toBeGreaterThan(0)
    expect(result.error).toBe(0)
  })

  test(`real ${name} hair uses distinct projected roots during combined head motion`, async ({ page }, testInfo) => {
    test.setTimeout(60_000)
    test.skip(!root, 'Set MEROPE_SHOULDER_ASSET / MEROPE_COLLAR_ASSET or MEROPE_HAIR_ASSETS to real split portraits')
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/hair-roots-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/hair-roots-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/hair-roots-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const { bindAttachmentMesh, sampleAttachmentMesh } = await import(`${modules}attachmentMesh.ts`)
      const original = JSON.stringify(manifest)
      const runs = []
      for (const previous of [true, false]) {
        const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
        await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/hair-roots-atlas.png')
        player.resize(768, 1024, 1)
        player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
        Object.assign(player.current, IDENTITY_DRIVER, { talk: false })
        const anchors = player.playback.anchors
        const commonSupport = () => (player.current.angleX * 14 + player.current.angleZ * 0.07 *
          (anchors.neckPivot.y - anchors.face.cy)) * anchors.faceScale + player.secondaryDeformationFrame.torsoNeckOffsetX
        if (previous) { for (const layer of player.layers) { for (const spring of layer.springs ?? []) {
          // Compare the previous common X estimate and its old output gains
          // (below). The current vertical channel and geometry stay identical;
          // this is an input/gain comparison, not the whole historical renderer.
          Object.defineProperty(spring, 'supportX', { get: commonSupport, set: () => {} })
        }
}
}
        const samples = player.layers.flatMap(layer => {
          if (!layer.hairRoots || !layer.hairSurface) return []
          return layer.source.strands.map((strand, i) => ({ layer: layer.source.name, bound: layer.hairRoots.samples[i], spring: layer.springs[i], sample: bindAttachmentMesh({
            rest: layer.rest, deformed: layer.hairSurface.base, indices: layer.indices,
          }, strand.x, strand.rootY) })).filter(item => item.sample)
        })
        const point = { x: 0, y: 0 }
        let rootError = 0; let rootSpread = 0
        let worstRoot = null
        let minAreaRatio = Infinity
        let maxEdgeRatio = 1
        let worstEdge = null
        let worstTriangle = null
        const area = (p, a, b, c) => (p[b] - p[a]) * (p[c + 1] - p[a + 1]) - (p[b + 1] - p[a + 1]) * (p[c] - p[a])
        const screenshots = []
        player.deform(); player.uploadGeometry(); player.draw()
        screenshots.push({ frame: 'neutral', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
        if (!previous) {
          // Static source-over atlas assembly bypasses every rig deformation.
          // It distinguishes split-art defects from motion/renderer defects.
          const atlasImage = new Image()
          atlasImage.src = '/hair-roots-atlas.png'
          await atlasImage.decode()
          const flat = document.createElement('canvas')
          flat.width = player.playback.pixelCanvas.width
          flat.height = player.playback.pixelCanvas.height
          const context = flat.getContext('2d')!
          for (const layer of player.layers) {
            const s = layer.source
            const uv = s.atlas
            context.globalAlpha = layer.frameOpacity
            context.drawImage(atlasImage, uv.x * atlasImage.width, uv.y * atlasImage.height,
              uv.w * atlasImage.width, uv.h * atlasImage.height, s.x, s.y, s.w, s.h)
          }
          screenshots.push({ frame: 'atlas-composite', image: flat.toDataURL('image/png').split(',')[1] })
        }
        for (let frame = 0; frame < 240; frame++) {
          const direction = frame < 80 ? 1 : frame < 160 ? -1 : 0
          player.setTarget({ ...IDENTITY_DRIVER, angleX: direction * 0.9, angleY: Math.sin(frame / 60 * Math.PI) * 0.85,
            angleZ: direction * 0.7, idle: false, rand: false, blink: false, talk: false, phys: true })
          player.time += 1 / 60
          player.smoothDriver(1 / 60); player.updateSprings(1 / 60)
          if (previous) { for (const layer of player.layers) { for (const spring of layer.springs ?? []) {
            spring.stiff.dx *= 2.2
            spring.soft.dx *= 3
          }
}
}
          player.deform()
          for (const layer of player.layers) {
            if (!layer.hairSurface) continue
            for (let i = 0; i < layer.indices.length; i += 3) {
              const [a, b, c] = Array.from(layer.indices.slice(i, i + 3), v => Number(v) * 2)
              const reference = area(layer.rest, a, b, c)
              if (Math.abs(reference) < 1e-8) continue
              const ratio = area(layer.deformed, a, b, c) / reference
              if (ratio < minAreaRatio) {
                minAreaRatio = ratio
                worstTriangle = { layer: layer.source.name, frame, indices: [a, b, c],
                  baseRatio: area(layer.hairSurface.base, a, b, c) / reference }
              }
              for (const [u, v] of [[a, b], [b, c], [c, a]]) {
                const base = layer.hairSurface.base
                const p = layer.deformed
                const length = Math.hypot(base[u] - base[v], base[u + 1] - base[v + 1])
                const stretch = Math.hypot(p[u] - p[v], p[u + 1] - p[v + 1]) / length
                if (stretch > maxEdgeRatio) {
                  maxEdgeRatio = stretch
                  worstEdge = { layer: layer.source.name, frame, indices: [u, v], length }
                }
              }
            }
          }
          const supports = samples.map(({ spring, sample, layer, bound }) => {
            sampleAttachmentMesh(sample, point)
            // The collar envelope can redirect excess pitch into body motion,
            // even when the requested body is zero. Include the shader-owned
            // rotation when comparing the mesh with world-space root inputs.
            const { bodyPivotX, bodyPivotY, bodyRotationCosine: cosine, bodyRotationSine: sine } = player.renderFrame
            const x = point.x - bodyPivotX; const y = point.y - bodyPivotY
            const worldX = bodyPivotX + x * cosine - y * sine
            const worldY = bodyPivotY + x * sine + y * cosine
            const error = Math.hypot(worldX - sample.x - spring.supportX, worldY - sample.y - spring.supportY)
            if (error > rootError) {
              rootError = error
              worstRoot = { layer, frame, x: sample.x, y: sample.y, probeIndices: sample.indices,
                boundIndices: bound.indices, probeWeights: sample.weights, boundWeights: bound.weights, body: player.current.body }
            }
            return spring.supportX
          })
          rootSpread = Math.max(rootSpread, Math.max(...supports) - Math.min(...supports))
          if (frame === 34 || frame === 87 || frame === 114) {
            player.uploadGeometry(); player.draw()
            screenshots.push({ frame, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
            if (!previous && frame === 87) {
              // Same physics state and pose, with only edge-length projection
              // removed; the pre-existing area barrier is still active.
              const savedEdges = player.layers.map(layer => layer.hairSurface?.edges)
              player.layers.forEach(layer => { if (layer.hairSurface) layer.hairSurface.edges = new Uint16Array() })
              player.deform(); player.uploadGeometry(); player.draw()
              screenshots.push({ frame: '87-area-only', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
              player.layers.forEach((layer, i) => { if (layer.hairSurface) layer.hairSurface.edges = savedEdges[i] })
              const physics = player.current.phys
              player.current.phys = false
              player.deform(); player.uploadGeometry(); player.draw()
              screenshots.push({ frame: '87-primary-only', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
              const blend = player.shellProfile.blend
              player.shellProfile.blend = 1
              player.deform(); player.uploadGeometry(); player.draw()
              screenshots.push({ frame: '87-shell-only', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
              player.shellProfile.blend = blend
              player.current.phys = physics
              player.deform()
              const face = player.layers.find(layer => layer.source.role === 'face')
              const opacity = face.frameOpacity
              face.frameOpacity = 0
              player.uploadGeometry(); player.draw()
              screenshots.push({ frame: '87-without-face', image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
              face.frameOpacity = opacity
            }
          }
        }
        runs.push({ previous, rootError, rootSpread, worstRoot, minAreaRatio, worstTriangle, maxEdgeRatio, worstEdge, samples: samples.length, error: player.gl.getError(), screenshots })
        player.dispose()
      }
      return { runs, unchanged: original === JSON.stringify(manifest) }
    }, { manifest, modules })
    await testInfo.attach('hair-root-metrics', { body: JSON.stringify({ ...result, runs: result.runs.map(({ screenshots: _screenshots, ...run }) => run) }, null, 2), contentType: 'application/json' })
    for (const run of result.runs) { for (const shot of run.screenshots) await testInfo.attach(`${run.previous ? 'common' : 'projected'}-${shot.frame}`, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
}
    const [previous, current] = result.runs
    expect(result.unchanged).toBe(true)
    expect(current.samples).toBeGreaterThan(5)
    expect(previous.rootSpread).toBe(0)
    expect(current.rootSpread).toBeGreaterThan(5)
    expect(previous.rootError).toBeGreaterThan(5)
    expect(current.rootError, JSON.stringify(current.worstRoot)).toBeLessThan(0.001)
    expect(current.minAreaRatio, JSON.stringify(current.worstTriangle)).toBeGreaterThan(0)
    // The solver's 1.25 local target is soft: adjacent area corrections can
    // relax it. This measured replay bound catches rubber-strip deformation.
    expect(current.maxEdgeRatio, JSON.stringify(current.worstEdge)).toBeLessThan(1.4)
    for (const run of result.runs) expect(run.error).toBe(0)
  })

  test(`real ${name} hair trails a body-only movement and settles on the player clock`, async ({ page }, testInfo) => {
    test.setTimeout(60_000)
    test.skip(!root, 'Set MEROPE_SHOULDER_ASSET / MEROPE_COLLAR_ASSET or MEROPE_HAIR_ASSETS to real split portraits')
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/hair-follow-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/hair-follow-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/hair-follow-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
      await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/hair-follow-atlas.png')
      player.resize(768, 1024, 1)
      player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
      Object.assign(player.current, IDENTITY_DRIVER, { talk: false })
      player.setTarget({ ...IDENTITY_DRIVER, body: 1, idle: false, blink: false, rand: false, talk: false, phys: true })
      let peakLag = 0; let peakSupport = 0; let visibleLag = 0
      const screenshots = []
      for (let frame = 0; frame < 900; frame++) {
        player.time += 1 / 60
        player.smoothDriver(1 / 60)
        player.updateSprings(1 / 60)
        const springs = player.layers.flatMap(layer => layer.springs ?? [])
        if (!springs.length) throw new Error('Fixture needs hair springs')
        if (Math.abs(player.current.angleX) > 1e-8 || Math.abs(player.current.angleZ) > 1e-8) throw new Error('Probe must isolate body movement')
        peakSupport = Math.max(peakSupport, ...springs.map(spring => spring.supportX))
        peakLag = Math.min(peakLag, ...springs.map(spring => spring.stiff.dx))
        if (frame === 15 || frame === 30) {
          // With idle wind and local head angles both zero, the previous loop
          // produced zero hair displacement. Draw that exact no-response case.
          player.current.phys = false
          player.deform(); player.uploadGeometry(); player.draw()
          screenshots.push({ name: `previous-${frame}`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
          player.current.phys = true
          player.deform(); player.uploadGeometry(); player.draw()
          screenshots.push({ name: `following-${frame}`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
          for (const layer of player.layers) {
            if (!layer.hairSurface) continue
            for (let i = 0; i < layer.deformed.length; i += 2) {
              if ((layer.secondaryDeformation.alongStrand?.[i / 2] ?? 0) < 0.4) continue
              visibleLag = Math.min(visibleLag, layer.deformed[i] - layer.hairSurface.base[i])
            }
          }
        }
      }
      const residual = Math.max(...player.layers.flatMap(layer => (layer.springs ?? []).flatMap(spring => [Math.abs(spring.stiff.dx), Math.abs(spring.soft.dx)])))
      const error = player.gl.getError()
      player.dispose()
      return { peakSupport, peakLag, visibleLag, residual, error, screenshots }
    }, { manifest, modules })
    await testInfo.attach('hair-follow-metrics', { body: JSON.stringify({ ...result, screenshots: undefined }, null, 2), contentType: 'application/json' })
    for (const shot of result.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
    expect(result.peakSupport).toBeGreaterThan(10)
    expect(result.peakLag).toBeLessThan(-5)
    expect(result.visibleLag).toBeLessThan(-3)
    expect(result.residual).toBeLessThan(0.2)
    expect(result.error).toBe(0)
  })
}
