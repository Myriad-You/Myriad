import type { Page } from '@playwright/test'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

async function mount(page: Page, saved: number, short = false) {
  await page.route('**/reader-progress', (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><div id="root"></div>',
    }),
  )
  await page.goto('/reader-progress')
  const fixture = `/@fs${fileURLToPath(new URL('./fixture/readerProgress.tsx', import.meta.url))}`
  await page.evaluate(
    async ({ fixture, saved, short }) => {
      ;(await import(fixture)).mountReaderProgress(saved, short)
    },
    { fixture, saved, short },
  )
  await page.getByRole('button', { name: 'load content' }).click()
  // Let pinning and the browser's programmatic scroll events finish.
  await settleScroll(page)
}

async function settleScroll(page: Page) {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        )
      }),
  )
}

async function flushProgress(page: Page) {
  await settleScroll(page)
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      value: 'hidden',
    })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  await settleScroll(page)
}

for (const width of [390, 1440]) {
  test(`completed article opens at zero and saves subsequent scrolling at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 })
    const writes: number[] = []
    await page.route('**/api/phantasi/sync-states', async (route) => {
      writes.push(route.request().postDataJSON().states[0].read_progress)
      await route.fulfill({
        json: { synced: 1, conflicts: [], revisions: { 23: 2 } },
      })
    })
    await mount(page, 100)
    await expect(page.getByTestId('progress')).toHaveText('0%')
    expect(
      await page.getByTestId('article').evaluate((el) => el.scrollTop),
    ).toBe(0)
    await flushProgress(page)
    await page.getByRole('button', { name: 'adopt remote' }).focus()
    expect(writes).toEqual([])

    await page.getByTestId('article').hover()
    await page.mouse.wheel(0, 500)
    await expect(page.getByTestId('progress')).toHaveText('29%')
    await flushProgress(page)
    await expect.poll(() => writes.at(-1)).toBe(29)
  })
}

test('partial progress is restored without a save', async ({ page }) => {
  const writes: unknown[] = []
  await page.route('**/api/phantasi/sync-states', async (route) => {
    writes.push(route.request().postDataJSON())
    await route.fulfill({ json: { synced: 1, conflicts: [] } })
  })
  await mount(page, 45)
  await expect(page.getByTestId('progress')).toHaveText('45%')
  expect(await page.getByTestId('article').evaluate((el) => el.scrollTop)).toBe(
    765,
  )
  await flushProgress(page)
  await page.getByRole('button', { name: 'adopt remote' }).focus()
  expect(writes).toEqual([])
})

for (const remote of [60, 100]) {
  test(`adopting remote progress ${remote} updates the position without overwriting it`, async ({
    page,
  }) => {
    const writes: unknown[] = []
    await page.route('**/api/phantasi/items/23', (route) =>
      route.fulfill({
        json: { item: { id: 23, read_progress: remote, state_revision: 2 } },
      }),
    )
    await page.route('**/api/phantasi/sync-states', async (route) => {
      writes.push(route.request().postDataJSON())
      await route.fulfill({ json: { synced: 1, conflicts: [] } })
    })
    await mount(page, 45)
    await page.getByRole('button', { name: 'adopt remote' }).click()
    const displayed = remote === 100 ? 0 : remote
    await expect(page.getByTestId('progress')).toHaveText(`${displayed}%`)
    await expect
      .poll(() => page.getByTestId('article').evaluate((el) => el.scrollTop))
      .toBe((displayed / 100) * 1700)
    await flushProgress(page)
    await page.getByRole('button', { name: 'load content' }).focus()
    expect(writes).toEqual([])
    expect(
      await page.evaluate(() =>
        JSON.parse(sessionStorage.getItem('phantasi:progress-outbox') || '[]'),
      ),
    ).toEqual([])
    await page.getByTestId('article').hover()
    await page.mouse.wheel(0, 500)
    const scrolled = Math.round(displayed + (500 / 1700) * 100)
    await expect(page.getByTestId('progress')).toHaveText(`${scrolled}%`)
    await flushProgress(page)
    await expect.poll(() => writes.length).toBe(1)
  })
}

test('an article that fits in the viewport displays complete progress', async ({
  page,
}) => {
  await mount(page, 0, true)
  await expect(page.getByTestId('progress')).toHaveText('100%')
})
