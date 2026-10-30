import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

for (const [name, assetVariable] of [['off-shoulder', 'MEROPE_SHOULDER_ASSET'], ['high-collar', 'MEROPE_COLLAR_ASSET']] as const) {
  test(`real ${name} torso carries head, hair and accessories through continuous turns`, async ({ page }, testInfo) => {
    test.setTimeout(60_000)
    const root = process.env[assetVariable]
    test.skip(!root, `Set ${assetVariable} to a real split portrait`)
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../../figure/src/anime25drig/', import.meta.url))}`
    await page.route('**/torso-carry-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/torso-carry-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/torso-carry-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const metrics = []
      const screenshots = []
      for (const fps of [30, 60, 120]) {
        const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
        await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/torso-carry-atlas.png')
        player.resize(768, 1024, 1)
        let enabled = true; let rootOffset = 0
        // Isolate the old missing-parent behavior without changing the torso,
        // clocks, input trajectory, texture, or any production feature flag.
        Object.defineProperty(player.secondaryDeformationFrame, 'torsoNeckOffsetX', {
          get: () => enabled ? rootOffset : 0,
          set: value => { rootOffset = value },
        })
        let headError = 0; let bodyError = 0; let attachmentError = 0; let extent = 0
        let maxStep = 0; let previous = 0
        for (let frame = 0; frame <= fps * 3; frame++) {
          const t = frame / fps
          const yaw = Math.sin(t * Math.PI * 2 / 3)
          player.setTarget({ ...IDENTITY_DRIVER, angleX: yaw, angleY: yaw * 0.6,
            angleZ: -yaw * 0.7, body: yaw * 0.8, bodyYaw: 1,
            idle: false, rand: false, blink: false, phys: true })
          player.time += 1 / fps
          player.smoothDriver(1 / fps)
          player.updateSprings(1 / fps)
          enabled = false
          player.deform()
          const before = player.layers.map(layer => ({ points: layer.deformed.slice(), matrix: layer.layerTransform.slice() }))
          if (fps === 60 && (frame === 60 || frame === 120)) {
            player.uploadGeometry(); player.draw()
            screenshots.push({ name: `previous-${frame}`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
          }
          enabled = true
          player.deform()
          extent = Math.max(extent, Math.abs(rootOffset))
          maxStep = Math.max(maxStep, Math.abs(rootOffset - previous)); previous = rootOffset
          player.layers.forEach((layer, i) => {
            if (layer.source.group === 'head' && !layer.shaderGlobalTransform && layer.frameOpacity > 0.01) {
              for (let p = 0; p < layer.deformed.length; p += 2) {
                headError = Math.max(headError, Math.abs(layer.deformed[p] - before[i].points[p] - rootOffset), Math.abs(layer.deformed[p + 1] - before[i].points[p + 1]))
              }
            }
            if (layer.attachment && layer.source.group === 'head' && layer.attachment.hostSource?.group === 'head') {
              attachmentError = Math.max(attachmentError, Math.abs(layer.layerTransform[6] - before[i].matrix[6] - rootOffset))
            }
            if (['topwear', 'bottomwear', 'handwear'].includes(layer.source.role)) {
              for (let p = 0; p < layer.deformed.length; p++) bodyError = Math.max(bodyError, Math.abs(layer.deformed[p] - before[i].points[p]))
            }
          })
          if (fps === 60 && (frame === 60 || frame === 120)) {
            player.uploadGeometry(); player.draw()
            screenshots.push({ name: `carried-${frame}`, image: player.gl.canvas.toDataURL('image/png').split(',')[1] })
          }
        }
        metrics.push({ fps, headError, bodyError, attachmentError, extent, maxStep, glError: player.gl.getError() })
        player.dispose()
      }
      return { metrics, screenshots }
    }, { manifest, modules })
    await testInfo.attach('torso-parent-metrics', { body: JSON.stringify(result.metrics, null, 2), contentType: 'application/json' })
    for (const shot of result.screenshots) await testInfo.attach(shot.name, { body: Buffer.from(shot.image, 'base64'), contentType: 'image/png' })
    for (const metric of result.metrics) {
      expect(metric.headError).toBeLessThan(0.01)
      expect(metric.attachmentError).toBeLessThan(0.1)
      expect(metric.bodyError).toBe(0)
      expect(metric.extent).toBeGreaterThan(10)
      expect(metric.maxStep).toBeLessThan(100 / metric.fps)
      expect(metric.glError).toBe(0)
    }
  })

  test(`real ${name} facial surface keeps eyes and mouth registered during large turns`, async ({ page }, testInfo) => {
    test.setTimeout(60_000)
    const root = process.env[assetVariable]
    test.skip(!root, `Set ${assetVariable} to a real split portrait`)
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../../figure/src/anime25drig/', import.meta.url))}`
    await page.route('**/face-surface-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/face-surface-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/face-surface-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const { deformAnime25DSecondaryPoint } = await import(`${modules}secondaryDeformation.ts`)
      const playback = manifest.anime25dPlayback
      const original = JSON.stringify(playback)
      const player = new Anime25DPlayer(document.querySelector('canvas'), playback, manifest)
      player.resize(768, 1024, 1)
      await player.replaceLivePackage(playback, manifest, '/face-surface-atlas.png')
      player.shellActivation = 1
      const features = player.layers.filter(layer => layer.secondaryDeformation.facialSurface)
      const face = features.find(layer => layer.source.role === 'face')
      if (!face || features.length < 6) throw new Error('Real face and split features required')
      const poses = [
        { angleX: 0, angleY: 0, angleZ: 0 },
        { angleX: 1, angleY: -0.85, angleZ: 0.8 },
        { angleX: -1, angleY: 0.85, angleZ: -0.8 },
        { angleX: 1, angleY: 0.85, eyeOpenL: 0, eyeOpenR: 0, mouthOpen: 0.6 },
        { angleX: -1, angleY: -0.85, eyeOpenL: 0.35, eyeOpenR: 0.35, mouthOpen: 0.5 },
      ]
      const runs = []
      for (const shared of [false, true, false]) {
        for (const layer of features) layer.secondaryDeformation.facialSurface = shared
        const frames = []
        for (const pose of poses) {
          Object.assign(player.current, IDENTITY_DRIVER, pose, { idle: false, blink: false, rand: false, phys: false })
          player.time = 1
          player.deform()
          player.uploadGeometry()
          player.draw()
          const pixels = new Uint8Array(768 * 1024 * 4)
          player.gl.readPixels(0, 0, 768, 1024, player.gl.RGBA, player.gl.UNSIGNED_BYTE, pixels)
          let registrationError = 0
          for (const layer of features) {
            const x = layer.source.x + layer.source.w / 2
            const y = layer.source.y + layer.source.h / 2
            const a = { x, y }; const b = { x, y }
            deformAnime25DSecondaryPoint(a, x, y, 0, face.secondaryDeformation, player.secondaryDeformationFrame)
            deformAnime25DSecondaryPoint(b, x, y, 0, layer.secondaryDeformation, player.secondaryDeformationFrame)
            registrationError = Math.max(registrationError, Math.hypot(a.x - b.x, a.y - b.y))
          }
          frames.push({ pixels, registrationError,
            protectedGeometry: player.layers.filter(layer => !features.includes(layer)).map(layer => Array.from(layer.deformed)),
            screenshot: player.gl.canvas.toDataURL('image/png').split(',')[1],
          })
        }
        runs.push(frames)
      }
      const differences = poses.map((_, i) => {
        const before = runs[0][i]; const after = runs[1][i]; const restored = runs[2][i]
        let changed = 0; let restoreError = 0; let protectedError = 0
        for (let p = 0; p < before.pixels.length; p += 4) {
          if (before.pixels.subarray(p, p + 4).some((v, k) => v !== after.pixels[p + k])) changed++
          if (before.pixels.subarray(p, p + 4).some((v, k) => v !== restored.pixels[p + k])) restoreError++
        }
        before.protectedGeometry.forEach((points, j) => points.forEach((v, k) => {
          protectedError = Math.max(protectedError, Math.abs(v - after.protectedGeometry[j][k]))
        }))
        return { changed, restoreError, protectedError, beforeError: before.registrationError, afterError: after.registrationError }
      })
      const screenshots = runs.slice(0, 2).map(frames => frames.map(frame => frame.screenshot))
      const unchangedAsset = original === JSON.stringify(playback)
      const error = player.gl.getError()
      player.dispose()
      return { differences, screenshots, unchangedAsset, error }
    }, { manifest, modules })
    await testInfo.attach('face-surface-metrics', { body: JSON.stringify(result.differences, null, 2), contentType: 'application/json' })
    for (const [run, frames] of result.screenshots.entries()) { for (const [pose, screenshot] of frames.entries()) {
      await testInfo.attach(`${run ? 'shared' : 'previous'}-face-${pose}`, { body: Buffer.from(screenshot, 'base64'), contentType: 'image/png' })
    }
}
    expect(result.error).toBe(0)
    expect(result.unchangedAsset).toBe(true)
    for (const [index, difference] of result.differences.entries()) {
      expect(difference.afterError).toBeLessThan(1e-6)
      expect(difference.protectedError).toBe(0)
      expect(difference.restoreError).toBe(0)
      if (index === 0) { expect(difference.changed).toBe(0)
}
      else {
        expect(difference.beforeError).toBeGreaterThan(1)
        expect(difference.changed).toBeGreaterThan(100)
      }
    }
  })
}

test('real asset combination corrections reach pixels, release exactly and preserve body geometry', async ({ page }, testInfo) => {
  test.setTimeout(60_000)
  const root = process.env.MEROPE_SHOULDER_ASSET
  test.skip(!root, 'Set MEROPE_SHOULDER_ASSET to a real split portrait')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../../figure/src/anime25drig/', import.meta.url))}`
  await page.route('**/correction-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/correction-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/correction-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
    const { poseCorrectionPatch } = await import(`${modules}poseCorrections.ts`)
    const { isAnime25DPlayback } = await import(`${modules}types.ts`)
    const { bindAttachmentMesh, sampleAttachmentMesh } = await import(`${modules}attachmentMesh.ts`)
    const original = manifest.anime25dPlayback
    const authored = structuredClone(original)
    const eye = original.anchors.eyeL
    if (!eye) throw new Error('Fixture must have an eye anchor')
    // Deliberately visible calibration stroke. It proves authoring reaches the
    // renderer, NOT that this arbitrary stroke is the artistically correct pose.
    const patch = poseCorrectionPatch(original.shellProfile.head,
      { x: eye.icx, y: eye.closeY }, { x: 0, y: 0 }, { x: 0, y: -16 }, { x: 110, y: 95 })
    authored.shellProfile.poseCorrections = [{ surface: 'head',
      at: { angleX: 0.8, angleY: -0.6, eyeCloseL: 1 }, patches: [patch] }]
    if (!isAnime25DPlayback(authored)) throw new Error('Authored package failed validation')
    const player = new Anime25DPlayer(document.querySelector('canvas'), original, manifest)
    player.resize(768, 1024, 1)
    const poses = [
      { angleX: 0, angleY: 0, eyeOpenL: 0, eyeOpenR: 0 },
      { angleX: 0.8, angleY: 0, eyeOpenL: 0, eyeOpenR: 0 },
      { angleX: 0, angleY: -0.6, eyeOpenL: 0, eyeOpenR: 0 },
      { angleX: 0.8, angleY: -0.6, eyeOpenL: 1, eyeOpenR: 1 },
      { angleX: 0.8, angleY: -0.6, eyeOpenL: 0, eyeOpenR: 0 },
      { angleX: 0.8, angleY: 0, eyeOpenL: 0, eyeOpenR: 0 },
    ]
    const runs = []
    for (const playback of [original, authored, original]) {
      await player.replaceLivePackage(playback, manifest, '/correction-atlas.png')
      player.shellActivation = 1
      const frames = []
      for (const pose of poses) {
        Object.assign(player.current, IDENTITY_DRIVER, pose, { idle: false, rand: false, blink: false, phys: false })
        player.time = 1
        player.deform()
        player.uploadGeometry()
        player.draw()
        const pixels = new Uint8Array(768 * 1024 * 4)
        player.gl.readPixels(0, 0, 768, 1024, player.gl.RGBA, player.gl.UNSIGNED_BYTE, pixels)
        const geometry = player.layers.map(layer => ({ role: layer.source.role, head: layer.source.group === 'head', points: Array.from(layer.deformed) }))
        const face = player.layers.find(layer => layer.source.role === 'face')
        const witness = bindAttachmentMesh(face, eye.icx, eye.closeY)
        const point = { x: 0, y: 0 }
        if (!witness) throw new Error('Eye anchor must bind to final face mesh')
        sampleAttachmentMesh(witness, point)
        const screenshot = player.gl.canvas.toDataURL('image/png').split(',')[1]
        player.deform()
        const dirty = player.layers.filter(layer => layer.geometryDirty)
        if (dirty.length) throw new Error(`Repeated evaluation marked geometry dirty: ${JSON.stringify(dirty.map(layer => ({ name: layer.source.name, opacity: layer.frameOpacity, correction: Boolean(layer.secondaryDeformation.poseCorrections) })))}`)
        frames.push({ geometry, pixels, point, screenshot })
      }
      runs.push(frames)
    }
    const differences = poses.map((_, poseIndex) => {
      const before = runs[0][poseIndex]
      const after = runs[1][poseIndex]
      const cleared = runs[2][poseIndex]
      let changedPixels = 0
      let restoredPixels = 0
      let bodyError = 0
      for (let i = 0; i < before.pixels.length; i += 4) {
        if (before.pixels.slice(i, i + 4).some((v, k) => v !== after.pixels[i + k])) changedPixels++
        if (before.pixels.slice(i, i + 4).some((v, k) => v !== cleared.pixels[i + k])) restoredPixels++
      }
      before.geometry.forEach((layer, index) => {
        if (!layer.head) layer.points.forEach((v, i) => { bodyError = Math.max(bodyError, Math.abs(v - after.geometry[index].points[i])) })
      })
      return { changedPixels, restoredPixels, bodyError, eyeSurfaceShift: Math.hypot(after.point.x - before.point.x, after.point.y - before.point.y) }
    })
    const screenshots = [runs[0][4].screenshot, runs[1][4].screenshot]
    const error = player.gl.getError()
    player.dispose()
    return { differences, screenshots, error }
  }, { manifest, modules })
  await testInfo.attach('combination-correction-differences', { body: JSON.stringify(result.differences), contentType: 'application/json' })
  for (const [index, screenshot] of result.screenshots.entries()) {
    await testInfo.attach(index ? 'authored-combination' : 'baseline-combination', { body: Buffer.from(screenshot, 'base64'), contentType: 'image/png' })
  }
  expect(result.error).toBe(0)
  result.differences.forEach((difference, index) => {
    expect(difference.bodyError).toBe(0)
    expect(difference.restoredPixels).toBe(0)
    if (index === 4) {
      expect(difference.changedPixels).toBeGreaterThan(100)
      expect(difference.eyeSurfaceShift).toBeGreaterThan(5)
      expect(difference.eyeSurfaceShift).toBeLessThan(8.1)
    } else {
      expect(difference.changedPixels).toBe(0)
      expect(difference.eyeSurfaceShift).toBe(0)
    }
  })
})

test('real high collar retains its aperture and does not become a skin contact', async ({ page }, testInfo) => {
  test.setTimeout(60_000)
  const root = process.env.MEROPE_COLLAR_ASSET
  test.skip(!root, 'Set MEROPE_COLLAR_ASSET to a genuine high-collar package')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../../figure/src/anime25drig/', import.meta.url))}`
  await page.route('**/collar-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
  await page.route('**/collar-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
  await page.goto('/collar-probe')
  const result = await page.evaluate(async ({ manifest, modules }) => {
    const { Anime25DPlayer } = await import(`${modules}player.ts`)
    const { buildAnime25DLayerBinding } = await import(`${modules}layerBinding.ts`)
    const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
    await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/collar-atlas.png')
    player.resize(768, 1024, 1)
    const protectedLayers = player.layers.filter(layer => ['neck', 'collar-front', 'collar-back'].includes(layer.source.role))
    const roles = protectedLayers.map(layer => layer.source.role)
    const contacts = protectedLayers.filter(layer => layer.surfaceContact).length
    // Covered sleeves in this fixture must retain every original vertex and UV.
    const sleeves = player.layers.filter(layer => layer.source.role === 'handwear')
    const unchangedSleeves = sleeves.every(layer => {
      const binding = buildAnime25DLayerBinding({ source: layer.source, canvasWidth: manifest.anime25dPlayback.pixelCanvas.width,
        face: manifest.anime25dPlayback.anchors.face, layerZ: layer.source.z ?? 0 })
      return !layer.surfaceContact && ['rest', 'atlasUvs', 'indices'].every(key =>
        binding[key].length === layer[key].length && binding[key].every((v, i) => v === layer[key][i]))
    })
    const clip = player.collarClip
    if (!clip) throw new Error('Genuine collar fixture did not create a neck aperture')
    const initial = clip.deformed.slice()
    let excursion = 0
    for (let frame = 0; frame < 120; frame++) {
      const t = frame / 60
      player.setTarget({ bodyYaw: Math.sin(t * 2.3), body: Math.sin(t * 1.7), angleX: Math.sin(t * 1.5),
        angleY: Math.cos(t * 1.9) * 0.7, angleZ: Math.sin(t * 2.1) * 0.7, idle: false, rand: false, blink: false })
      player.time += 1 / 60
      player.smoothDriver(1 / 60)
      player.updateSprings(1 / 60)
      player.deform()
      if (!clip.deformed.every(Number.isFinite)) throw new Error('Invalid collar aperture geometry')
      player.uploadGeometry()
      for (let i = 0; i < initial.length; i++) excursion = Math.max(excursion, Math.abs(clip.deformed[i] - initial[i]))
    }
    player.draw()
    const screenshot = player.gl.canvas.toDataURL('image/png').split(',')[1]
    let uploadedError = 0
    // A high collar draws the aperture mesh instead of the retired neck grid.
    for (const layer of [clip, ...protectedLayers]) {
      if (!layer.vertexBuffer) continue
      const uploaded = new Float32Array(layer.deformed.length)
      player.gl.bindBuffer(player.gl.ARRAY_BUFFER, layer.vertexBuffer)
      player.gl.getBufferSubData(player.gl.ARRAY_BUFFER, 0, uploaded)
      for (let i = 0; i < uploaded.length; i++) uploadedError = Math.max(uploadedError, Math.abs(uploaded[i] - layer.deformed[i]))
    }
    player.gl.bindBuffer(player.gl.ARRAY_BUFFER, null)
    const error = player.gl.getError()
    player.dispose()
    return { roles, contacts, sleeves: sleeves.length, unchangedSleeves, excursion, uploadedError, screenshot, error }
  }, { manifest, modules })
  expect(result.roles).toEqual(expect.arrayContaining(['neck', 'collar-front', 'collar-back']))
  expect(result.contacts).toBe(0)
  expect(result.sleeves).toBe(2)
  expect(result.unchangedSleeves).toBe(true)
  expect(result.excursion).toBeGreaterThan(1)
  expect(result.uploadedError).toBeLessThan(0.0001)
  expect(result.error).toBe(0)
  await testInfo.attach('collar-upload-metrics', { body: JSON.stringify({ uploadedError: result.uploadedError, excursion: result.excursion }), contentType: 'application/json' })
  await testInfo.attach('real-high-collar', { body: Buffer.from(result.screenshot, 'base64'), contentType: 'image/png' })
})

test('real shoulder fusion keeps GPU coverage through body and arm motion', async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000)
  const root = process.env.MEROPE_SHOULDER_ASSET
  test.skip(
    !root,
    'Set MEROPE_SHOULDER_ASSET to a split shoulder fixture package',
  )
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(
    new URL('../../../figure/src/anime25drig/', import.meta.url),
  )}`
  await page.route('**/shoulder-probe', (route) =>
    route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }),
  )
  await page.route('**/shoulder-atlas.png', (route) =>
    route.fulfill({ contentType: 'image/png', body: atlas }),
  )
  await page.goto('/shoulder-probe')
  const result = await page.evaluate(
    async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const { deformAnime25DUpstreamFeaturePoint } = await import(`${modules}layerDeformation.ts`)
      const { deformAnime25DFaceJawPoint } = await import(`${modules}mouthDeformation.ts`)
      const { deformAnime25DSecondaryPoint } = await import(`${modules}secondaryDeformation.ts`)
      const { bindAttachmentMesh, sampleAttachmentMesh } = await import(`${modules}attachmentMesh.ts`)
      const { createAtlasTexture, readLayerPixels } = await import(`${modules}webglRuntime.ts`)
      const { intentExpressionPatch } = await import(
        `${modules}performanceCueDefinitions.ts`,
      )
      const { THINKING_EXPRESSION_PRESET } = await import(
        `${modules}expressionPresets.ts`,
      )
      const { SingingGrooveController } = await import(
        `${modules}../singing/singingGroove.ts`,
      )
      const { musicSignalAt } = await import(
        `${modules}../singing/musicSignal.test-support.ts`,
      )
      const player = new Anime25DPlayer(
        document.querySelector('canvas'),
        manifest.anime25dPlayback,
        manifest,
      )
      await player.replaceLivePackage(
        manifest.anime25dPlayback,
        manifest,
        '/shoulder-atlas.png',
      )
      player.resize(1024, 1400, 1)
      const gl = player.gl as WebGL2RenderingContext
      const fused = player.atlasTexture
      const image = new Image()
      image.src = '/shoulder-atlas.png'
      await image.decode()
      const headMeshes = player.layers
        .filter(layer => ['face', 'eyewhite', 'eyelash', 'eye-close', 'eye-close2'].includes(layer.source.role))
        .map(layer => {
          const raster = readLayerPixels(image, layer.source)
          const triangles: number[][] = []
          for (let i = 0; i < layer.indices.length; i += 3) {
            const vertices = Iterator.from(layer.indices.slice(i, i + 3))
              .map((v: number) => v * 2)
              .toArray()
            const x = vertices.reduce((n, v) => n + layer.rest[v], 0) / 3
            const y = vertices.reduce((n, v) => n + layer.rest[v + 1], 0) / 3
            const px = Math.floor((x - layer.source.x) / layer.source.w * raster.width)
            const py = Math.floor((y - layer.source.y) / layer.source.h * raster.height)
            if (px >= 0 && py >= 0 && px < raster.width && py < raster.height && raster.pixels[(py * raster.width + px) * 4 + 3] > 128) {
              triangles.push(vertices)
            }
          }
          const witnesses = []
          // Fixed texture-space witnesses are independent of mesh density.
          const stride = layer.source.role === 'face' ? 12 : 3
          for (let py = 1; py < raster.height; py += stride) {
            for (let px = 1; px < raster.width; px += stride) {
              if (raster.pixels[(py * raster.width + px) * 4 + 3] <= 128) continue
              const x = layer.source.x + (px + 0.5) / raster.width * layer.source.w
              const y = layer.source.y + (py + 0.5) / raster.height * layer.source.h
              const sample = bindAttachmentMesh(layer, x, y)
              if (sample) witnesses.push(sample)
            }
          }
          return { layer, triangles, witnesses }
        })
      const hairMeshes = player.layers.filter(layer => ['front-hair', 'back-hair'].includes(layer.source.role)).map(layer => {
        const raster = readLayerPixels(image, layer.source)
        const triangles = []
        for (let i = 0; i < layer.indices.length; i += 3) {
          const vertices = [...layer.indices.slice(i, i + 3)].map(v => v * 2)
          const x = vertices.reduce((n, v) => n + layer.rest[v], 0) / 3
          const y = vertices.reduce((n, v) => n + layer.rest[v + 1], 0) / 3
          const px = Math.floor((x - layer.source.x) / layer.source.w * raster.width)
          const py = Math.floor((y - layer.source.y) / layer.source.h * raster.height)
          if (px >= 0 && py >= 0 && px < raster.width && py < raster.height && raster.pixels[(py * raster.width + px) * 4 + 3] > 128) triangles.push(vertices)
        }
        return { layer, triangles }
      })
      const baseline = createAtlasTexture(gl, image)
      const width = gl.drawingBufferWidth
      const height = gl.drawingBufferHeight
      const read = () => {
        const pixels = new Uint8Array(width * height * 4)
        gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
        return pixels
      }
      const groove = new SingingGrooveController()
      groove.setTrack('shoulder-music-probe')
      groove.setArmMotion(true)
      let left = { ...groove.sample(0, true, musicSignalAt(0)) }
      let right = { ...left }
      for (let frame = 1; frame <= 60 * 30; frame++) {
        const pose = groove.sample(frame / 60, true, musicSignalAt(frame / 60))
        if (pose.body < left.body) left = { ...pose }
        if (pose.body > right.body) right = { ...pose }
      }
      const poses = [
        { bodyYaw: 0, body: 0, armY: 0, armPos: 0 },
        { bodyYaw: 1, body: 1, armY: 1, armPos: 1 },
        { bodyYaw: -1, body: -1, armY: -1, armPos: -1 },
        ...['greet', 'delight', 'emphasize'].map((intent) => ({
          bodyYaw: 0,
          body: 0,
          armY: 0,
          armPos: 0,
          angleY: 0,
          angleZ: 0,
          ...intentExpressionPatch(intent, 1.4),
        })),
        { ...left, bodyYaw: 0 },
        { ...right, bodyYaw: 0 },
        {
          bodyYaw: 0,
          body: 0,
          armY: 0,
          armPos: 0,
          ...THINKING_EXPRESSION_PRESET,
        },
        ...[-1, 1].flatMap(yaw => [-1, 1].flatMap(pitch => [1, 0].map(eyeOpen => ({
          angleX: yaw, angleY: pitch * 0.85, angleZ: yaw * 0.85,
          eyeOpenL: eyeOpen, eyeOpenR: eyeOpen,
        })))),
      ]
      const results = []
      const contacts = player.layers.filter((layer) => layer.surfaceContact)
      if (contacts.length !== 2) {
        throw new Error(
          `Expected two exposed shoulder contacts, got ${contacts.length}`,
        )
}
      for (const layer of contacts) {
        if (
          !layer.localDynamic ||
          layer.shaderGlobalTransform ||
          layer.deformationPlan.cacheable
        ) {
          throw new Error(
            'Shoulder contact requires freshly evaluated model-space geometry',
          )
        }
      }
      // Probe the torso's actual alpha edge inside the arm, not just vertices
      // the binder has itself labelled as fully pinned.
      const torso = player.layers.find(layer => layer.source.role === 'topwear')
      const bodyPixels = readLayerPixels(image, torso.source)
      const skinAt = (layer, raster, x, y) => {
        const px = Math.floor((x - layer.x) / layer.w * raster.width)
        const py = Math.floor((y - layer.y) / layer.h * raster.height)
        if (px < 0 || py < 0 || px >= raster.width || py >= raster.height) return false
        const i = (py * raster.width + px) * 4
        const [r, g, b, a] = raster.pixels.slice(i, i + 4)
        return a > 220 && r > 100 && r > g + 3 && g > b - 12 && r - b > 8 && r - g < 85
      }
      const boundarySamples = contacts.flatMap(layer => {
        const raster = readLayerPixels(image, layer.source)
        const samples = []
        const fromLeft = layer.source.x < torso.source.x + torso.source.w / 2
        for (let py = 0; py < bodyPixels.height; py += 4) {
          const y = torso.source.y + (py + 0.5) / bodyPixels.height * torso.source.h
          if (y < layer.source.y || y > layer.source.y + layer.source.h * 0.5) continue
          let edge = -1
          for (let n = 0; n < bodyPixels.width; n++) {
            const px = fromLeft ? n : bodyPixels.width - 1 - n
            if (bodyPixels.pixels[(py * bodyPixels.width + px) * 4 + 3] > 220) { edge = px; break }
          }
          if (edge < 0) continue
          const x = torso.source.x + (edge + 0.5) / bodyPixels.width * torso.source.w
          if (!skinAt(layer.source, raster, x, y) || !skinAt(torso.source, bodyPixels, x + (fromLeft ? 4 : -4), y)) continue
          const armSample = bindAttachmentMesh(layer, x, y)
          const bodySample = bindAttachmentMesh(torso, x, y)
          if (armSample && bodySample) samples.push({ armSample, bodySample })
        }
        if (samples.length < 3) throw new Error(`No independent shoulder edge evidence for ${layer.source.name}`)
        return samples
      })
      // Exercise the real controller and geometry continuously, not only settled endpoints.
      // Read the host triangles independently of the contact evaluator.
      const sweeps = []
      const seamError = (layer) => {
        let maximum = 0
        for (
          let vertex = 0;
          vertex < layer.surfaceContact.samples.length;
          vertex++
        ) {
          // Feather vertices retain deliberate independent motion, even at .9999.
          if (layer.surfaceContact.weights[vertex] !== 1) continue
          const sample = layer.surfaceContact.samples[vertex]
          const mesh = sample.mesh
          let x = sample.x
          let y = sample.y
          for (let k = 0; k < 3; k++) {
            const i = sample.indices[k] * 2
            x += (mesh.deformed[i] - mesh.rest[i]) * sample.weights[k]
            y += (mesh.deformed[i + 1] - mesh.rest[i + 1]) * sample.weights[k]
          }
          const m = mesh.transform
          const expectedX = m ? m[0] * x + m[3] * y + m[6] : x
          const expectedY = m ? m[1] * x + m[4] * y + m[7] : y
          maximum = Math.max(
            maximum,
            Math.hypot(
              layer.deformed[vertex * 2] - expectedX,
              layer.deformed[vertex * 2 + 1] - expectedY,
            ),
          )
        }
        return maximum
      }
      const area = (points, a, b, c) =>
        (points[b] - points[a]) * (points[c + 1] - points[a + 1]) -
        (points[b + 1] - points[a + 1]) * (points[c] - points[a])
      for (const fps of [30, 60, 120]) {
        let maxSeamError = 0
        let maxUnboundSeamError = 0
        let minAreaRatio = Infinity
        let redundantDirty = 0
        let maxIdempotenceError = 0
        let maxBoundaryError = 0
        let minHairAreaRatio = Infinity
        let worstHair = null
        const deformationTimes = []
        for (let frame = 0; frame < fps * 4; frame++) {
          const t = frame / fps
          player.setTarget({
            bodyYaw: Math.sin(t * 2.3),
            body: Math.sin(t * 1.7),
            armY: Math.sin(t * 2.7),
            armPos: Math.cos(t * 1.3),
            angleX: Math.sin(t * 1.5),
            angleY: Math.cos(t * 1.9) * 0.7,
            angleZ: Math.sin(t * 2.1) * 0.7,
            idle: false,
            rand: false,
            blink: false,
          })
          player.time += 1 / fps
          player.smoothDriver(1 / fps)
          player.updateSprings(1 / fps)
          const deformationStart = performance.now()
          player.deform()
          deformationTimes.push(performance.now() - deformationStart)
          for (const { layer, triangles } of hairMeshes) {
            for (const [a, b, c] of triangles) {
              const ratio = area(layer.deformed, a, b, c) / area(layer.rest, a, b, c)
              if (ratio < minHairAreaRatio) {
                minHairAreaRatio = ratio
                worstHair = { name: layer.source.name, frame, triangle: [a, b, c], baseRatio: area(layer.hairSurface.base, a, b, c) / area(layer.rest, a, b, c) }
              }
            }
          }
          for (const { armSample, bodySample } of boundarySamples) {
            const armPoint = { x: 0, y: 0 }
            const bodyPoint = { x: 0, y: 0 }
            sampleAttachmentMesh(armSample, armPoint)
            sampleAttachmentMesh(bodySample, bodyPoint)
            maxBoundaryError = Math.max(maxBoundaryError, Math.hypot(armPoint.x - bodyPoint.x, armPoint.y - bodyPoint.y))
          }
          for (const layer of contacts) {
            maxSeamError = Math.max(maxSeamError, seamError(layer))
            for (let i = 0; i < layer.indices.length; i += 3) {
              const [a, b, c] = Iterator.from(layer.indices.slice(i, i + 3))
                .map((index: number) => index * 2)
                .toArray()
              minAreaRatio = Math.min(
                minAreaRatio,
                area(layer.deformed, a, b, c) / area(layer.rest, a, b, c),
              )
            }
          }
          const checkedLayers = [...contacts, ...hairMeshes.map(({ layer }) => layer)]
          const retained = checkedLayers.map((layer) => layer.deformed.slice())
          player.deform()
          checkedLayers.forEach((layer, n) => {
            if (layer.geometryDirty) redundantDirty++
            for (let i = 0; i < layer.deformed.length; i++) {
              maxIdempotenceError = Math.max(
                maxIdempotenceError,
                Math.abs(layer.deformed[i] - retained[n][i]),
              )
            }
          })
          const bindings = contacts.map((layer) => layer.surfaceContact)
          contacts.forEach((layer) => {
            layer.surfaceContact = undefined
          })
          player.deform()
          contacts.forEach((layer, i) => {
            layer.surfaceContact = bindings[i]
          })
          for (const layer of contacts) {
            maxUnboundSeamError = Math.max(
              maxUnboundSeamError,
              seamError(layer),
            )
}
        }
        sweeps.push({
          fps,
          maxSeamError,
          maxUnboundSeamError,
          minAreaRatio,
          redundantDirty,
          maxIdempotenceError,
          maxBoundaryError,
          minHairAreaRatio,
          worstHair,
          deformationP95Ms: deformationTimes.toSorted((a, b) => a - b)[Math.floor(deformationTimes.length * 0.95)],
        })
      }
      for (const pose of poses) {
        player.setTarget({ ...IDENTITY_DRIVER, ...pose, idle: false, rand: false, blink: false })
        // Advance the real control/physics path without issuing 90 redundant GPU draws.
        for (let i = 0; i < 90; i++) {
          player.time += 1 / 60
          player.smoothDriver(1 / 60)
          player.updateSprings(1 / 60)
        }
        player.atlasTexture = fused
        player.tick(1 / 60)
        const headGeometry = headMeshes
          .filter(({ layer }) => layer.frameOpacity > 0.01)
          .map(({ layer, triangles, witnesses }) => {
            const errors = witnesses.map(sample => {
              const { x, y } = sample
              const point = { x, y }
              if (layer.upstreamFeature) deformAnime25DUpstreamFeaturePoint(point, layer.upstreamFeature, player.irisRebound)
              if (layer.baseRole === 'face') deformAnime25DFaceJawPoint(point, y, player.deformationFrame)
              deformAnime25DSecondaryPoint(point, x, y, 0, layer.secondaryDeformation, player.secondaryDeformationFrame)
              const interpolated = { x: 0, y: 0 }
              sampleAttachmentMesh(sample, interpolated)
              return Math.hypot(point.x - interpolated.x, point.y - interpolated.y)
            })
            const maxInterpolationError = Math.max(...errors)
            const worst = witnesses[errors.indexOf(maxInterpolationError)]
            return {
              name: layer.source.name,
              triangles: triangles.length,
              minAreaRatio: Math.min(...triangles.map(([a, b, c]) => area(layer.deformed, a, b, c) / area(layer.rest, a, b, c))),
              witnesses: witnesses.length,
              maxInterpolationError,
              worstRest: worst ? [worst.x, worst.y] : null,
            }
          })
        const hairGeometry = hairMeshes.map(({ layer, triangles }) => {
          const ratios = triangles.map(([a, b, c]) => area(layer.deformed, a, b, c) / area(layer.rest, a, b, c))
          const minAreaRatio = Math.min(...ratios)
          const worst = triangles[ratios.indexOf(minAreaRatio)]
          return { name: layer.source.name, triangles: triangles.length, minAreaRatio,
            maxAreaRatio: Math.max(...ratios),
            worstRest: worst.map(v => [layer.rest[v], layer.rest[v + 1], layer.secondaryDeformation.hairlinePinWeights?.[v / 2] ?? 0]),
          }
        })
        const after = read()
        const screenshot = (gl.canvas as HTMLCanvasElement)
          .toDataURL('image/png')
          .split(',')[1]
        player.atlasTexture = baseline
        player.draw()
        const before = read()
        let holes = 0
        let changed = 0
        for (let y = 0; y < height; y++) {
          for (let x = 0; x < width; x++) {
            // Shoulder/torso region excludes the headwear component intentionally removed.
            if (y / height > 0.44) continue
            const i = (y * width + x) * 4
            if (after[i + 3] + 1 < before[i + 3]) holes++
            if (after[i] > before[i] + 8 && after[i + 1] > before[i + 1] + 8)
              changed++
          }
        }
        results.push({ holes, changed, error: gl.getError(), screenshot, headGeometry, hairGeometry })
      }
      gl.deleteTexture(baseline)
      player.atlasTexture = fused
      player.dispose()
      return { results, sweeps }
    },
    { manifest, modules },
  )
  await testInfo.attach('continuous-shoulder-geometry', {
    body: JSON.stringify(result.sweeps, null, 2),
    contentType: 'application/json',
  })
  for (const sweep of result.sweeps) {
    expect(sweep.maxSeamError).toBeLessThan(0.001)
    // Independent, texture-derived visible edge, including triangle interiors.
    expect(sweep.maxBoundaryError).toBeLessThan(0.25)
    expect(sweep.maxUnboundSeamError).toBeGreaterThan(1)
    expect(sweep.minAreaRatio).toBeGreaterThan(0)
    expect(sweep.minHairAreaRatio).toBeGreaterThan(0)
    expect(sweep.redundantDirty).toBe(0)
    expect(sweep.maxIdempotenceError).toBe(0)
  }
  for (const [index, pose] of result.results.entries()) {
    await testInfo.attach(`hair-geometry-${index}`, {
      body: JSON.stringify(pose.hairGeometry, null, 2), contentType: 'application/json',
    })
    expect(pose.hairGeometry.length).toBeGreaterThan(0)
    for (const surface of pose.hairGeometry) {
      expect(surface.triangles).toBeGreaterThan(0)
      expect(surface.minAreaRatio, `${index}: ${surface.name}`).toBeGreaterThan(0)
    }
    await testInfo.attach(`head-geometry-${index}`, {
      body: JSON.stringify(pose.headGeometry, null, 2), contentType: 'application/json',
    })
    expect(pose.headGeometry.some(surface => surface.name === 'face')).toBe(true)
    for (const surface of pose.headGeometry) {
      expect(surface.triangles, surface.name).toBeGreaterThan(0)
      expect(surface.witnesses, surface.name).toBeGreaterThan(0)
      expect(surface.maxInterpolationError, `${index}: ${surface.name}`).toBeLessThan(surface.name === 'face' ? 1 : 0.25)
      expect(surface.minAreaRatio, `${index}: ${surface.name}`).toBeGreaterThan(0)
    }
    await testInfo.attach(`shoulder-pose-${index}`, {
      body: Buffer.from(pose.screenshot, 'base64'),
      contentType: 'image/png',
    })
    expect(pose.error).toBe(0)
    expect(pose.holes).toBe(0)
    expect(pose.changed).toBeGreaterThan(100)
  }
})
