import { expect, test } from '@playwright/test'

test('relative source assets resolve against the page and preserve fetch errors', async ({
  page,
}) => {
  const requests: string[] = []
  await page.route('**/assets/master.png', (route) => {
    requests.push(new URL(route.request().url()).pathname)
    return route.fulfill({ status: 404, body: '' })
  })
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.relativeSourceFailure(),
  )
  expect(requests).toEqual(['/assets/master.png'])
  expect(result.error).toBe(result.expected)
})

test.beforeEach(async ({ page }) => {
  // A self-contained fixture server only; never talk to an actual backend.
  await page.route('**/api/**', (route) => route.abort())
  await page.goto('/rigImport.html')
  await page.waitForFunction(() => 'rigImportTest' in window)
})

for (const kind of ['ordinary', 'collar', 'necklace', 'alternate-eyes']) {
  test(`real worker import preserves ${kind} manifest and PNG pixels`, async ({
    page,
  }) => {
    const result = await page.evaluate(async (kind) => {
      const harness = (window as any).rigImportTest
      return harness.run(kind)
    }, kind)
    expect(result.error).toBeUndefined()
    expect(result.stages).toEqual(['validated', 'packing'])
    expect(result.sourceEqual).toBe(true)
    expect(result.partCount).toBeGreaterThan(15)
    expect(result.atlas).toEqual(result.expectedAtlas)
    expect(result.reference).toEqual(result.expectedReference)
    if (kind === 'collar') expect(result.roles).toContain('collar-front')
    if (kind === 'necklace') {
      expect(result.roles).toContain('neckwear')
      expect(result.roles).not.toContain('collar-front')
    }
  })
}

test('real player distinguishes automatic blinks, deliberate closure and special eyes', async ({
  page,
}) => {
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.eyeRuntime(),
  )
  expect(result.ordinaryBlink).toBeGreaterThan(0.9)
  expect(result.alternateBlink).toBe(0)
  expect(result.wink.left).toBeGreaterThan(0.95)
  expect(result.wink.ordinaryLeft).toBeLessThan(0.01)
  expect(result.wink.right).toBeLessThan(0.01)
  expect(result.both.left).toBeGreaterThan(0.95)
  expect(result.both.right).toBeGreaterThan(0.95)
  expect(result.cry).toBeLessThan(0.01)
  expect(result.rebound).toBeGreaterThan(0.003)
  expect(result.rebound).toBeLessThan(0.045)
  expect(result.glError).toBe(0)
})

test('packing cancellation rejects and a fresh import still succeeds', async ({
  page,
}) => {
  const result = await page.evaluate(async () => {
    const harness = (window as any).rigImportTest
    return {
      cancelled: await harness.run('ordinary', true),
      next: await harness.run('ordinary'),
    }
  })
  expect(result.cancelled.stages).toEqual(['validated', 'packing'])
  expect(result.cancelled.name).toBe('AbortError')
  expect(result.next.error).toBeUndefined()
  expect(result.next.sourceEqual).toBe(true)
})

test('worker compile failures keep the selected UI language', async ({
  page,
}) => {
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.localizedFailure(),
  )
  expect(result.expected).toMatch(/[\u4E00-\u9FFF]/)
  expect(result.result.error).toBe(result.expected)
})
