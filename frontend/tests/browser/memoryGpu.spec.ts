import { writeFile } from 'node:fs/promises'
import { expect, test } from '@playwright/test'

const empty = { Buffer: 0, Texture: 0, Program: 0, Shader: 0, VertexArray: 0 }
const noBytes = { buffers: 0, textures: 0 }

test.describe.configure({ timeout: 90_000 })

test.beforeEach(async ({ page }) => {
  await page.goto('/memoryGpu.html')
  await page.waitForFunction(() => Boolean(window.memoryGpu))
})

for (const failure of ['', 'upload', 'constructor']) {
  test(`WebGL resources balance across repeated ${failure || 'successful'} loads`, async ({ page }) => {
    const result = await page.evaluate(f => window.memoryGpu.gpuCycles(5, f), failure)
    expect(result.rejected).toBe(failure ? 5 : 0)
    expect(result.unchanged).toBe(true)
    expect(result.glError).toBe(0)
    expect(result.final).toEqual(empty)
    expect(result.finalBytes).toEqual(noBytes)
    for (const snapshot of result.snapshots) {
      if ('active' in snapshot) {
        expect(snapshot.afterReplacement).toEqual(snapshot.active)
        expect(snapshot.afterReplacementBytes).toEqual(snapshot.activeBytes)
      }
      else {
        expect(snapshot).toEqual(empty)
      }
    }
  })
}

test('repeated glass and avatar lifecycles release DOM and GPU resources across engines', async ({ page }, testInfo) => {
  const samples = []
  for (let batch = 0; batch < 4; batch++) {
    const result = await page.evaluate(async () => {
      const glass = await window.memoryGpu.glassCycles(20)
      for (let i = 0; i < 5; i++) {
        await window.memoryGpu.mountCharacter()
        window.memoryGpu.unmountCharacter()
      }
      return { glass, resources: window.memoryGpu.characterResources(), canvases: document.querySelectorAll('canvas').length, rigs: document.querySelectorAll('.merope-rig').length }
    })
    if (result.glass.supported) {
      expect(result.glass.largestMap).toBeGreaterThan(0)
    }
    else {
      expect(result.glass.largestMap).toBe(0)
      expect(result.glass.fallbackStyle).toContain('blur(')
    }
    expect(result.glass.filters).toBe(0)
    expect(result.canvases).toBe(0)
    expect(result.rigs).toBe(0)
    expect(result.resources.counts).toEqual(empty)
    expect(result.resources.bytes).toEqual(noBytes)
    samples.push(result)
  }
  const path = testInfo.outputPath('dom-gpu-release.json')
  await writeFile(path, JSON.stringify(samples, null, 2))
  await testInfo.attach('dom-gpu-release', { path, contentType: 'application/json' })
})

test('4096px atlas replacement bounds estimated upload bytes and releases them', async ({ page, browserName }, testInfo) => {
  const result = await page.evaluate(() => window.memoryGpu.gpuCycles(3, '', 4096))
  const atlasBytes = 4096 * 4096 * 4
  // Iris draw keeps one RGBA eye-mask target the size of the backing store.
  // Replacement may hold both atlases plus that mask, but not a third atlas.
  const maskBytes = result.drawingBufferBytes
  expect(result.renderer.maxTextureSize).toBeGreaterThanOrEqual(4096)
  expect(result.glError).toBe(0)
  expect(result.final).toEqual(empty)
  expect(result.finalBytes).toEqual(noBytes)
  expect(maskBytes).toBeGreaterThan(0)
  expect(maskBytes).toBeLessThan(atlasBytes)
  expect(result.peakBytes.textures).toBe(2 * atlasBytes + maskBytes)
  let steadyBuffers = 0
  for (const snapshot of result.snapshots) {
    if (!('active' in snapshot)) continue
    expect(snapshot.activeBytes.textures).toBe(atlasBytes + maskBytes)
    expect(snapshot.afterReplacementBytes).toEqual(snapshot.activeBytes)
    steadyBuffers = Math.max(steadyBuffers, snapshot.activeBytes.buffers)
  }
  expect(steadyBuffers).toBeGreaterThan(0)
  expect(result.peakBytes.buffers).toBeLessThanOrEqual(steadyBuffers * 2)
  const path = testInfo.outputPath('atlas-upload-estimates.json')
  await writeFile(path, JSON.stringify({ browserName, ...result }, null, 2))
  await testInfo.attach('atlas-upload-estimates', { path, contentType: 'application/json' })
})

test('Chromium-only collected heap stabilizes after repeated lifecycles', async ({ page, browserName }, testInfo) => {
  test.skip(browserName !== 'chromium', 'CDP heap collection is Chromium-only; WebKit is covered by DOM and GL release checks')
  test.setTimeout(90_000)
  const cdp = await page.context().newCDPSession(page)
  const samples = []
  for (let batch = 0; batch < 4; batch++) {
    const gpu = await page.evaluate(() => window.memoryGpu.gpuCycles(5))
    expect(gpu.final).toEqual(empty)
    const glass = await page.evaluate(() => window.memoryGpu.glassCycles(20))
    expect(glass.largestMap).toBeGreaterThan(0)
    expect(glass.filters).toBe(0)
    expect(glass.canvases).toBe(0)
    await page.evaluate(async () => {
      for (let i = 0; i < 5; i++) {
        await window.memoryGpu.mountCharacter()
        window.memoryGpu.unmountCharacter()
      }
    })
    await cdp.send('HeapProfiler.collectGarbage')
    samples.push({ heap: await cdp.send('Runtime.getHeapUsage'), dom: await cdp.send('Memory.getDOMCounters') })
  }
  const path = testInfo.outputPath('chromium-collected-heap.json')
  await writeFile(path, JSON.stringify(samples, null, 2))
  await testInfo.attach('memory-samples', { path, contentType: 'application/json' })
  expect(samples[3].dom.nodes).toBeLessThanOrEqual(samples[1].dom.nodes + 5)
  expect(samples[3].dom.jsEventListeners).toBe(samples[1].dom.jsEventListeners)
  // A broad retained-growth guard after warm-up, not a platform-independent heap budget.
  expect(samples[3].heap.usedSize - samples[1].heap.usedSize).toBeLessThan(1024 * 1024)
})

test('the mounted character stops offscreen, resumes, recovers context loss and stops on unmount', async ({ page }, testInfo) => {
  await page.evaluate(() => window.memoryGpu.mountCharacter())
  const start = await page.evaluate(() => window.memoryGpu.ticks())
  await expect.poll(() => page.evaluate(() => window.memoryGpu.ticks())).toBeGreaterThan(start + 2)
  await page.evaluate(() => window.memoryGpu.offscreen(true))
  // Allow delivery of the real IntersectionObserver before observing inactivity.
  await page.waitForTimeout(150)
  const hidden = await page.evaluate(() => window.memoryGpu.ticks())
  await page.waitForTimeout(150)
  expect(await page.evaluate(() => window.memoryGpu.ticks())).toBe(hidden)
  await page.evaluate(() => window.memoryGpu.offscreen(false))
  await expect.poll(() => page.evaluate(() => window.memoryGpu.ticks())).toBeGreaterThan(hidden + 2)
  await page.evaluate(() => {
    const canvas = document.querySelector('canvas')!
    canvas.setAttribute('data-old-context', 'true')
    canvas.getContext('webgl2')!.getExtension('WEBGL_lose_context')!.loseContext()
  })
  await expect(page.locator('canvas[data-old-context]')).toHaveCount(0)
  await expect(page.locator('.merope-rig.is-ready')).toHaveCount(1)
  const recovered = await page.evaluate(() => window.memoryGpu.characterResources())
  expect(recovered.contexts).toBe(2)
  expect(recovered.counts.Texture).toBeGreaterThan(0)
  await page.evaluate(() => window.memoryGpu.unmountCharacter())
  const stopped = await page.evaluate(() => window.memoryGpu.ticks())
  await page.waitForTimeout(150)
  expect(await page.evaluate(() => window.memoryGpu.ticks())).toBe(stopped)
  await expect(page.locator('canvas')).toHaveCount(0)
  const final = await page.evaluate(() => window.memoryGpu.characterResources())
  expect(final.counts).toEqual(empty)
  expect(final.bytes).toEqual(noBytes)
  const path = testInfo.outputPath('context-recovery-release.json')
  await writeFile(path, JSON.stringify({ recovered, final }, null, 2))
  await testInfo.attach('context-recovery-release', { path, contentType: 'application/json' })
})
