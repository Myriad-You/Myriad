import { expect, test } from '@playwright/test'

interface PerformanceFixture {
  renders: () => number
  publish: (detail: Record<string, unknown>) => void
  sharedEventManager: {
    add: (event: string, callback: () => void, options: { throttle: boolean }) => () => void
    clear: () => void
    getStats: () => Record<string, number>
  }
}

declare global {
  interface Window {
    performanceFixture: PerformanceFixture
  }
}

test.beforeEach(async ({ page }) => {
  await page.route('**/api/**', route => route.fulfill({
    contentType: 'application/json',
    body: JSON.stringify({ success: true, data: {} }),
  }))
  await page.goto('/performance.html')
  await expect(page.locator('#music')).toHaveText('false:-1')
})

test('music consumers ignore duplicate and progress-only snapshots, but receive actual changes', async ({ page }) => {
  const result = await page.evaluate(async () => {
    const fixture = window.performanceFixture
    const settle = () => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
    await settle()
    const before = fixture.renders()
    for (let index = 0; index < 100; index++) {
      fixture.publish({ isPlaying: false, currentLyricIndex: -1 })
      fixture.publish({ currentTime: index, audioDuration: 200 })
    }
    await settle()
    return { before, after: fixture.renders() }
  })
  expect(result.after).toBe(result.before)
  await page.evaluate(() => window.performanceFixture.publish({ isPlaying: true, currentLyricIndex: 3 }))
  await expect(page.locator('#music')).toHaveText('true:3')
  await page.evaluate(() => window.performanceFixture.publish({ isPlaying: false }))
  await expect(page.locator('#music')).toHaveText('false:3')
})

test('removed throttled listeners cannot deliver an old frame to a new subscriber', async ({ page }) => {
  const result = await page.evaluate(async () => {
    const manager = window.performanceFixture.sharedEventManager
    const event = 'performance-regression-event'
    let calls = 0
    const remove = manager.add(event, () => {}, { throttle: true })
    window.dispatchEvent(new Event(event))
    remove()
    const removeNext = manager.add(event, () => calls++, { throttle: true })
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()))
    const staleCalls = calls
    window.dispatchEvent(new Event(event))
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()))
    removeNext()
    return { staleCalls, calls, remaining: manager.getStats()[event] ?? 0 }
  })
  expect(result).toEqual({ staleCalls: 0, calls: 1, remaining: 0 })
})
