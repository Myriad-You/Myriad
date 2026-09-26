import { expect, test } from '@playwright/test'

test.beforeEach(async ({ page }) => {
  await page.goto('/mediaSource.html')
  await page.waitForFunction(() => !!window.mediaSourceTest)
})

test('site media at an old origin is displayed through the current site', async ({ page }) => {
  await page.evaluate(() =>
    window.mediaSourceTest.show('https://old.invalid/api/media/7/content?v=2'),
  )
  await expect(page.locator('img')).toHaveAttribute(
    'src',
    /\/api\/media\/7\/content\?v=2$/,
  )
  const src = await page.locator('img').getAttribute('src')
  expect(src).not.toContain('old.invalid')
})

test('public catalog assets keep their path and empty sources stay empty', async ({ page }) => {
  await page.evaluate(() => window.mediaSourceTest.show('/media/assets/public.png'))
  await expect(page.locator('img')).toHaveAttribute(
    'src',
    /\/media\/assets\/public\.png$/,
  )
  await page.evaluate(() => window.mediaSourceTest.show(undefined))
  await expect(page.locator('img')).toHaveCount(0)
  await expect(page.locator('.is-empty')).toHaveCount(1)
})
