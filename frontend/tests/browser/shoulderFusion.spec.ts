import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

test('real high collar retains its aperture and does not become a skin contact', async ({ page }, testInfo) => {
  test.setTimeout(60_000)
  const root = process.env.MEROPE_COLLAR_ASSET
  test.skip(!root, 'Set MEROPE_COLLAR_ASSET to a genuine high-collar package')
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const atlas = await readFile(`${root}/atlas.png`)
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
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
      for (let i = 0; i < initial.length; i++) excursion = Math.max(excursion, Math.abs(clip.deformed[i] - initial[i]))
    }
    player.draw()
    const screenshot = player.gl.canvas.toDataURL('image/png').split(',')[1]
    const error = player.gl.getError()
    player.dispose()
    return { roles, contacts, sleeves: sleeves.length, unchangedSleeves, excursion, screenshot, error }
  }, { manifest, modules })
  expect(result.roles).toEqual(expect.arrayContaining(['neck', 'collar-front', 'collar-back']))
  expect(result.contacts).toBe(0)
  expect(result.sleeves).toBe(2)
  expect(result.unchangedSleeves).toBe(true)
  expect(result.excursion).toBeGreaterThan(1)
  expect(result.error).toBe(0)
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
    new URL('../../src/features/merope/anime25drig/', import.meta.url),
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
              minHairAreaRatio = Math.min(minHairAreaRatio, area(layer.deformed, a, b, c) / area(layer.rest, a, b, c))
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
