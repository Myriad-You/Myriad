import { Buffer } from 'node:buffer'
import { expect, test } from '@playwright/test'

test('choosing a private wallpaper keeps it private until the configuration is saved', async ({ page }) => {
  const mutations: string[] = []
  await page.route('**/api/media**', async route => {
    const request = route.request()
    if (request.method() !== 'GET') mutations.push(request.method())
    if (request.url().includes('/7/content')) {
      await route.fulfill({ contentType: 'image/png', body: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=', 'base64') })
    } else {
      await route.fulfill({ json: { items: [{ id: 7, name: 'Private wallpaper', url: '/api/media/7/content', content_path: '/api/media/7/content', mime: 'image/png', kind: 'upload', references: [], state: 'ready', exposure: 'private' }], next_cursor: null } })
    }
  })
  await page.goto('/wallpaperMedia.html')
  await page.locator('button[aria-expanded]').click()
  await expect.poll(() => page.locator('img').evaluate(img => (img as HTMLImageElement).naturalWidth)).toBe(2)
  await page.getByRole('button', { name: 'Private wallpaper', exact: true }).click()
  await expect(page.locator('output')).toHaveText('/api/media/7/content')
  expect(mutations).toEqual([])
  await expect(page.locator('button[aria-expanded]')).toHaveAttribute('aria-expanded', 'false')
})
