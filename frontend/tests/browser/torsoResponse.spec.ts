import { Buffer } from 'node:buffer'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

test('body posture GPU projection agrees with CPU picking and support sampling', async ({ page }) => {
  const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
  await page.goto('/tests/browser/fixture/rigImport.html')
  const error = await page.evaluate(async (modules) => {
    const { BODY_LIFT_GLSL, applyBodyLift } = await import(`${modules}bodyLift.ts`)
    const gl = document.createElement('canvas').getContext('webgl2')!
    const compile = (kind, code) => {
      const shader = gl.createShader(kind)!
      gl.shaderSource(shader, code); gl.compileShader(shader)
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader)!)
      return shader
    }
    const program = gl.createProgram()!
    const vs = compile(gl.VERTEX_SHADER, `#version 300 es\nin vec2 p; out vec2 result; uniform vec4 f; uniform vec3 pose;\n${BODY_LIFT_GLSL}\nvoid main(){result=bodyLift(bodyPitch(p,f,pose),f);gl_Position=vec4(0.,0.,0.,1.);}`)
    const fs = compile(gl.FRAGMENT_SHADER, '#version 300 es\nprecision highp float;out vec4 color;void main(){color=vec4(1.);}')
    gl.attachShader(program, vs); gl.attachShader(program, fs)
    gl.transformFeedbackVaryings(program, ['result'], gl.INTERLEAVED_ATTRIBS)
    gl.linkProgram(program)
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program)!)
    gl.useProgram(program)
    const input = Float32Array.from(Array.from({ length: 51 }, (_, i) => [200 + i * 11, 200 + i * 24]).flat())
    const vao = gl.createVertexArray(); gl.bindVertexArray(vao)
    const buffer = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buffer); gl.bufferData(gl.ARRAY_BUFFER, input, gl.STATIC_DRAW)
    const loc = gl.getAttribLocation(program, 'p'); gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0)
    const output = gl.createBuffer(); gl.bindBuffer(gl.TRANSFORM_FEEDBACK_BUFFER, output); gl.bufferData(gl.TRANSFORM_FEEDBACK_BUFFER, input.byteLength, gl.DYNAMIC_READ)
    gl.bindBufferBase(gl.TRANSFORM_FEEDBACK_BUFFER, 0, output)
    const values = new Float32Array(input.length)
    let maximum = 0
    gl.enable(gl.RASTERIZER_DISCARD)
    for (const pitch of [-0.18, 0, 0.18]) { for (const amount of [-25, 0, 25]) {
      const field = { centerX: 500, upperY: 800, lowerY: 1200, amount, pitch, depth: 220, shoulderY: 600 }
      gl.uniform4f(gl.getUniformLocation(program, 'f'), 500, 800, 1200, amount)
      gl.uniform3f(gl.getUniformLocation(program, 'pose'), pitch, 220, 600)
      gl.beginTransformFeedback(gl.POINTS); gl.drawArrays(gl.POINTS, 0, input.length / 2); gl.endTransformFeedback()
      gl.getBufferSubData(gl.TRANSFORM_FEEDBACK_BUFFER, 0, values)
      for (let i = 0; i < input.length; i += 2) {
        const p = { x: input[i], y: input[i + 1] }; applyBodyLift(p, field)
        maximum = Math.max(maximum, Math.abs(values[i] - p.x), Math.abs(values[i + 1] - p.y))
      }
    }
}
    if (gl.getError()) throw new Error('GPU posture replay failed')
    gl.deleteBuffer(buffer); gl.deleteBuffer(output); gl.deleteVertexArray(vao)
    gl.deleteProgram(program); gl.deleteShader(vs); gl.deleteShader(fs)
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    return maximum
  }, modules)
  expect(error).toBeLessThan(0.001)
})

for (const [name, variable] of [['off-shoulder', 'MEROPE_SHOULDER_ASSET'], ['high-collar', 'MEROPE_COLLAR_ASSET']] as const) {
  test(`real ${name} body lift changes pixels with physics off and preserves the shared upper structure`, async ({ page }, testInfo) => {
    const root = process.env[variable]
    test.skip(!root, `Set ${variable}`)
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/lift-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/lift-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/lift-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const { applyBodyLift } = await import(`${modules}bodyLift.ts`)
      const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
      await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/lift-atlas.png')
      player.resize(600, 800, 1)
      player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
      const poses = []; const shots = []
      for (const [lift, pitch] of [[0, 0], [1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [-1, -1]]) {
        player.setTarget({ ...IDENTITY_DRIVER, bodyLift: lift, bodyPitch: pitch, phys: false, idle: false, blink: false, rand: false, mouse: false, talk: false })
        for (let i = 0; i < 120; i++) player.tick(1 / 60)
        const field = player.renderFrame.bodyLift
        const neck = { x: manifest.anime25dPlayback.anchors.neckPivot.x, y: manifest.anime25dPlayback.anchors.neckBottom }
        const before = { ...neck }
        applyBodyLift(neck, field)
        const cut = { x: field.centerX + 100, y: field.lowerY }
        applyBodyLift(cut, field)
        const gl = player.gl; const pixels = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4)
        gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
        const amount = field.amount
        const pitchAmount = field.pitch
        field.amount = 0
        field.pitch = 0
        player.draw()
        const baseline = new Uint8Array(pixels.length)
        gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, baseline)
        field.amount = amount
        field.pitch = pitchAmount
        player.draw()
        let changed = 0
        for (let j = 0; j < pixels.length; j += 4) { if (Math.abs(pixels[j] - baseline[j]) + Math.abs(pixels[j + 1] - baseline[j + 1]) + Math.abs(pixels[j + 2] - baseline[j + 2]) > 12) changed++
}
        let hash = 0
        for (const v of pixels) hash = (Math.imul(hash, 31) + v) | 0
        let minAreaRatio = Infinity
        for (const layer of player.layers) {
          if (layer.frameOpacity < 0.01) continue
          const m = layer.layerTransform; const v = layer.deformed
          const before = new Float32Array(v.length); const after = new Float32Array(v.length)
          for (let j = 0; j < v.length; j += 2) {
            const point = { x: m[0] * v[j] + m[3] * v[j + 1] + m[6], y: m[1] * v[j] + m[4] * v[j + 1] + m[7] }
            before[j] = point.x; before[j + 1] = point.y
            applyBodyLift(point, field)
            after[j] = point.x; after[j + 1] = point.y
          }
          const area = (p, a, b, c) => (p[b] - p[a]) * (p[c + 1] - p[a + 1]) - (p[b + 1] - p[a + 1]) * (p[c] - p[a])
          for (let j = 0; j < layer.indices.length; j += 3) {
            const [a, b, c] = [layer.indices[j] * 2, layer.indices[j + 1] * 2, layer.indices[j + 2] * 2]
            const baseArea = area(before, a, b, c)
            if (Math.abs(baseArea) > 0.01) minAreaRatio = Math.min(minAreaRatio, area(after, a, b, c) / baseArea)
          }
        }
        poses.push({ lift, pitch, amount: field.amount, pitchAmount, neckShift: neck.y - before.y, cutShift: cut.y - field.lowerY, hash, changed, minAreaRatio })
        shots.push({ name: `${lift}-${pitch}`, data: gl.canvas.toDataURL('image/png').split(',')[1] })
      }
      // Existing head/body performances must recruit this field without callers
      // knowing about the new explicit posture channels.
      player.setTarget({ ...IDENTITY_DRIVER, angleY: 0.8, phys: false, idle: false, blink: false, rand: false, mouse: false, talk: false })
      for (let i = 0; i < 120; i++) player.tick(1 / 60)
      const recruited = { ...player.renderFrame.bodyLift }
      const error = player.gl.getError()
      player.dispose()
      return { poses, shots, recruited, error }
    }, { manifest, modules })
    for (const shot of result.shots) await testInfo.attach(`body-lift-${shot.name}`, { body: Buffer.from(shot.data, 'base64'), contentType: 'image/png' })
    await testInfo.attach('body-lift-metrics', { body: JSON.stringify(result.poses), contentType: 'application/json' })
    expect(result.error).toBe(0)
    expect(result.recruited.amount).toBeGreaterThan(5)
    expect(result.recruited.pitch).toBeGreaterThan(0.04)
    expect(result.poses[1].amount).toBeGreaterThan(8)
    expect(result.poses[2].amount).toBeLessThan(-8)
    expect(result.poses[1].changed).toBeGreaterThan(1000)
    expect(result.poses[2].changed).toBeGreaterThan(1000)
    expect(new Set(result.poses.map(p => p.hash)).size).toBe(7)
    expect(result.poses[3].pitchAmount).toBeGreaterThan(0.17)
    expect(result.poses[4].pitchAmount).toBeLessThan(-0.17)
    for (const p of result.poses) {
      if (!p.pitch) expect(Math.abs(p.neckShift + p.amount)).toBeLessThan(0.001)
      if (p.pitch || p.lift) expect(p.changed).toBeGreaterThan(1000)
      expect(p.cutShift).toBe(0)
      expect(p.minAreaRatio).toBeGreaterThan(0.65)
    }
  })
  test(`real ${name} torso responds to short turns through the player driver`, async ({ page }, testInfo) => {
    test.setTimeout(60_000)
    const root = process.env[variable]
    test.skip(!root, `Set ${variable} to a real split portrait`)
    const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const atlas = await readFile(`${root}/atlas.png`)
    const modules = `/@fs${fileURLToPath(new URL('../../src/features/merope/anime25drig/', import.meta.url))}`
    await page.route('**/torso-response-probe', route => route.fulfill({ contentType: 'text/html', body: '<canvas></canvas>' }))
    await page.route('**/torso-response-atlas.png', route => route.fulfill({ contentType: 'image/png', body: atlas }))
    await page.goto('/torso-response-probe')
    const result = await page.evaluate(async ({ manifest, modules }) => {
      const { Anime25DPlayer } = await import(`${modules}player.ts`)
      const { IDENTITY_DRIVER } = await import(`${modules}driver.ts`)
      const { anime25DTorsoYawFollow } = await import(`${modules}torsoDeformation.ts`)
      const runs = []
      const original = JSON.stringify(manifest)
      for (const previous of [true, false]) {
        const player = new Anime25DPlayer(document.querySelector('canvas'), manifest.anime25dPlayback, manifest)
        await player.replaceLivePackage(manifest.anime25dPlayback, manifest, '/torso-response-atlas.png')
        player.resize(768, 1024, 1)
        player.setMotionPolicy({ mouth: 'preview', expression: 'preview', gaze: 'preview', headBody: 'preview' })
        Object.assign(player.current, IDENTITY_DRIVER)
        let oldYaw = 0
        let atCueEnd = 0
        let rootAtCueEnd = 0
        let screenshot = ''
        // Same authored cue, same driver easing and physics. Only substitute
        // the previous torso filter in the reference run, never in production.
        for (let frame = 0; frame < 180; frame++) {
          player.setTarget({ ...IDENTITY_DRIVER, angleX: frame < 18 ? 1 : 0, idle: false, rand: false, blink: false, phys: true })
          player.time += 1 / 60
          player.smoothDriver(1 / 60)
          if (previous) {
            const follow = anime25DTorsoYawFollow(player.shellProfile.torso, player.current.bodyYaw)
            const target = player.current.angleX * 0.45 * follow + player.current.body * 0.1
            oldYaw += (target - oldYaw) * (2.5 / 60)
            Object.assign(player.torsoYaw, { value: oldYaw, velocity: 0 })
            Object.assign(player.torsoShellRotation, { active: Math.abs(oldYaw) > 1e-7, yawCosine: Math.cos(oldYaw), yawSine: Math.sin(oldYaw) })
          }
          player.updateSprings(1 / 60)
          if (frame === 17) {
            player.deform(); player.uploadGeometry(); player.draw()
            atCueEnd = player.torsoYaw.value
            rootAtCueEnd = Math.abs(player.secondaryDeformationFrame.torsoNeckOffsetX)
            screenshot = player.gl.canvas.toDataURL('image/png').split(',')[1]
          }
        }
        runs.push({ previous, atCueEnd, rootAtCueEnd, residual: Math.abs(player.torsoYaw.value), error: player.gl.getError(), screenshot })
        player.dispose()
      }
      return { runs, unchanged: original === JSON.stringify(manifest) }
    }, { manifest, modules })
    await testInfo.attach('torso-response-metrics', { body: JSON.stringify({ ...result, runs: result.runs.map(({ screenshot: _screenshot, ...metrics }) => metrics) }, null, 2), contentType: 'application/json' })
    for (const run of result.runs) await testInfo.attach(run.previous ? 'previous-follow' : 'responsive-follow', { body: Buffer.from(run.screenshot, 'base64'), contentType: 'image/png' })
    const [previous, current] = result.runs
    expect(result.unchanged).toBe(true)
    expect(previous.atCueEnd).toBeGreaterThan(0)
    expect(current.atCueEnd).toBeGreaterThan(previous.atCueEnd * 1.3)
    expect(current.rootAtCueEnd).toBeGreaterThan(previous.rootAtCueEnd * 1.2)
    for (const run of result.runs) {
      expect(run.error).toBe(0)
      expect(run.residual).toBeLessThan(0.001)
    }
  })
}
