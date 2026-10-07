import { expect, test } from '@playwright/test'
import { fileURLToPath } from 'node:url'

test.beforeEach(async ({ page }) => {
  await page.route('**/api/phantasi/items/*', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 500))
    await route.fulfill({
      json: {
        item: { id: 1, source_id: 1, title: 'external', content: 'body' },
      },
    })
  })
  await page.route('**/journal/**', (route) => {
    if (route.request().resourceType() === 'document') {
      return route.fulfill({
        contentType: 'text/html',
        body: '<!doctype html><div id="root"></div>',
      })
    }
    return route.continue()
  })
  await page.goto('/journal/notes')
  const fixture = `/@fs${fileURLToPath(new URL('./fixture/journalReaderHistory.tsx', import.meta.url))}`
  await page.evaluate(
    async (fixture) => (await import(fixture)).mountReaderHistory(),
    fixture,
  )
  await expect(
    page.getByRole('button', { name: 'friends', exact: true }),
  ).toBeVisible()
})

for (const width of [390, 1440]) {
  test(`rapid external opens consume one history entry at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 })
    await page.getByRole('button', { name: 'friends', exact: true }).click()
    const before = await page.evaluate(() => history.length)
    await page
      .getByRole('button', { name: 'external', exact: true })
      .evaluate((node) => {
        for (let i = 0; i < 3; i++) (node as HTMLButtonElement).click()
      })
    await expect(page.getByRole('dialog')).toBeVisible()
    await expect(page.getByTestId('opening')).toHaveText('false')
    await expect
      .poll(() => page.evaluate(() => history.length))
      .toBe(before + 1)
    await expect(page).toHaveURL(/\/journal\/friends$/)
    await page.goBack()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    await expect(page).toHaveURL(/\/journal\/friends$/)
    await page.goForward()
    await expect(page.getByRole('dialog')).toBeVisible()
    await page.getByRole('button', { name: 'close reader' }).click()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    await expect(page).toHaveURL(/\/journal\/friends$/)
    await page.goBack()
    await expect(page).toHaveURL(/\/journal\/notes$/)
  })
}

test('switching external articles replaces the reader entry', async ({
  page,
}) => {
  await page.getByRole('button', { name: 'friends', exact: true }).click()
  await page.getByRole('button', { name: 'external', exact: true }).click()
  await expect(page.getByTestId('item')).toHaveText('1')
  await page.getByRole('button', { name: 'next external' }).click()
  await expect(page.getByTestId('item')).toHaveText('3')
  await page.goBack()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(page).toHaveURL(/\/journal\/friends$/)
})

test('back during loading cancels the pending open', async ({ page }) => {
  await page.getByRole('button', { name: 'friends', exact: true }).click()
  await page.getByRole('button', { name: 'external', exact: true }).click()
  await expect(page.getByTestId('opening')).toHaveText('true')
  await page.goBack()
  await expect(page).toHaveURL(/\/journal\/notes$/)
  await page.waitForTimeout(750)
  await expect(page.getByRole('dialog')).toHaveCount(0)
})

test('own article still pushes its URL and closes with back', async ({
  page,
}) => {
  await page.getByRole('button', { name: 'own', exact: true }).click()
  await expect(page).toHaveURL(/\/journal\/articles\/2$/)
  await page.goBack()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(page).toHaveURL(/\/journal\/notes$/)
})

test('external deep link strips the item URL without inventing a previous page', async ({
  page,
}) => {
  await page.getByRole('button', { name: 'external deep link' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await expect(page).toHaveURL(/\/journal\/friends$/)
  await page.getByRole('button', { name: 'close reader' }).click()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(page).toHaveURL(/\/journal\/friends$/)
})

test('browser Forward restores a search result without looking up a database id', async ({
  page,
}) => {
  const requests: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('/api/phantasi/items/'))
      requests.push(request.url())
  })
  await page.getByRole('button', { name: 'search result' }).click()
  await expect(page.getByTestId('item')).toHaveText('-1')
  await page.goBack()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await page.goForward()
  await expect(page.getByTestId('item')).toHaveText('-1')
  expect(requests).toEqual([])
})

test('a cancelled deep-link request cannot roll back a newer article', async ({ page }) => {
  await page.getByRole('button', { name: 'external deep link' }).click()
  await expect(page.getByTestId('opening')).toHaveText('true')
  await page.getByRole('button', { name: 'next external' }).click()
  await expect(page.getByTestId('item')).toHaveText('3')
  await expect(page).toHaveURL(/\/journal\/friends$/)
  await page.waitForTimeout(750)
  await expect(page.getByTestId('item')).toHaveText('3')
})
