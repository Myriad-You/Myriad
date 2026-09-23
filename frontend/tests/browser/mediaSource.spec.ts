import { expect, test } from '@playwright/test'

test.beforeEach(async ({ page }) => {
  await page.goto('/mediaSource.html')
  await page.waitForFunction(() => !!window.mediaSourceTest)
})

test('late private reads cannot replace the current image or leak object URLs', async ({ page }) => {
  await page.evaluate(() => window.mediaSourceTest.show('/api/media/1/content?revision=1'))
  await expect.poll(() => page.evaluate(() => window.mediaSourceTest.requests.length)).toBe(1)
  await page.evaluate(() => window.mediaSourceTest.show('/api/media/2/content'))
  await expect.poll(() => page.evaluate(() => window.mediaSourceTest.requests.length)).toBe(2)
  await page.evaluate(() => window.mediaSourceTest.requests[1].finish())
  await expect(page.locator('img')).toHaveAttribute('src', /^blob:/)
  const current = await page.locator('img').getAttribute('src')
  await page.evaluate(async () => { window.mediaSourceTest.requests[0].finish(); await new Promise(resolve => setTimeout(resolve, 0)) })
  await expect(page.locator('img')).toHaveAttribute('src', current!)
  expect(await page.evaluate(() => window.mediaSourceTest.created.length)).toBe(1)
  await page.evaluate(() => window.mediaSourceTest.unmount())
  expect(await page.evaluate(() => window.mediaSourceTest.revoked)).toEqual([current])
})

test('switching sources clears the previous private preview immediately, including A-B-A', async ({ page }) => {
  await page.evaluate(() => window.mediaSourceTest.show('/api/media/1/content'))
  await expect.poll(() => page.evaluate(() => window.mediaSourceTest.requests.length)).toBe(1)
  await page.evaluate(() => window.mediaSourceTest.requests[0].finish())
  await expect(page.locator('img')).toHaveAttribute('src', /^blob:/)
  const previous = await page.locator('img').getAttribute('src')
  await page.evaluate(() => window.mediaSourceTest.show('/api/media/2/content'))
  await expect(page.locator('img')).toHaveCount(0)
  await page.evaluate(() => window.mediaSourceTest.show('/api/media/1/content'))
  await expect(page.locator('img')).toHaveCount(0)
  expect(await page.evaluate(() => window.mediaSourceTest.revoked)).toContain(previous)
  await page.evaluate(() => window.mediaSourceTest.show('/media/assets/public.png'))
  await expect(page.locator('img')).toHaveAttribute('src', '/media/assets/public.png')
  await page.evaluate(() => window.mediaSourceTest.show(undefined))
  await expect(page.locator('img')).toHaveCount(0)
})

test('a private catalog image with an old site origin loads through the current site', async ({ page }) => {
  await page.evaluate(() => window.mediaSourceTest.show('https://old.invalid/api/media/7/content?v=2'))
  await expect.poll(() => page.evaluate(() => window.mediaSourceTest.requests[0]?.url)).toBe('/api/media/7/content?v=2')
  await page.evaluate(() => window.mediaSourceTest.requests[0].finish())
  await expect.poll(() => page.locator('img').evaluate(img => (img as HTMLImageElement).naturalWidth)).toBe(2)
})
