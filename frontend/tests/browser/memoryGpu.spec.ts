import { expect, test } from '@playwright/test'

const empty = { Buffer: 0, Texture: 0, Program: 0, Shader: 0, VertexArray: 0 }

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
    for (const snapshot of result.snapshots) {
      if ('active' in snapshot) expect(snapshot.afterReplacement).toEqual(snapshot.active)
      else expect(snapshot).toEqual(empty)
    }
  })
}

test('repeated glass and avatar lifecycles release DOM and stabilize collected heap', async ({ page }, testInfo) => {
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
  await testInfo.attach('memory-samples', { body: JSON.stringify(samples, null, 2), contentType: 'application/json' })
  expect(samples[3].dom.nodes).toBeLessThanOrEqual(samples[1].dom.nodes + 5)
  expect(samples[3].dom.jsEventListeners).toBe(samples[1].dom.jsEventListeners)
  // A broad retained-growth guard after warm-up, not a platform-independent heap budget.
  expect(samples[3].heap.usedSize - samples[1].heap.usedSize).toBeLessThan(1024 * 1024)
})

test('the mounted character stops offscreen, resumes, recovers context loss and stops on unmount', async ({ page }) => {
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
  await page.evaluate(() => window.memoryGpu.unmountCharacter())
  const stopped = await page.evaluate(() => window.memoryGpu.ticks())
  await page.waitForTimeout(150)
  expect(await page.evaluate(() => window.memoryGpu.ticks())).toBe(stopped)
  await expect(page.locator('canvas')).toHaveCount(0)
})
