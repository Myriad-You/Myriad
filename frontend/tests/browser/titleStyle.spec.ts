import { expect, test } from '@playwright/test'

// Appearance saves authenticate through the shared API client, which reads the session's CSRF token.
const CSRF_TOKEN = `v1.${'a'.repeat(24)}.${'b'.repeat(43)}`

test.beforeEach(async ({ page }) => {
  await page.route('**/api/csrf-token', route => route.fulfill({ json: { csrf_token: CSRF_TOKEN, expires_in: 3600 } }))
})

test('a failed initial title read recovers on remount without reverting edits', async ({ page }) => {
  let fail = true
  await page.route('**/api/config/ui', route => route.fulfill({
    status: fail ? 400 : 200,
    json: fail ? { error: 'Read failed' } : { title_font_size: 0.8, title_color: 'accent' },
  }))
  await page.route('**/api/config/dashboard', route => route.fulfill({ json: {} }))
  const failed = page.waitForEvent('console', { predicate: message => message.text().includes('加载标题样式设置失败') })
  await page.goto('/titleStyle.html')
  await failed
  await page.getByRole('button', { name: 'Change size', exact: true }).click()
  fail = false
  await page.getByRole('button', { name: 'Toggle reader', exact: true }).click()
  await expect(page.locator('[data-color]')).toHaveText('accent')
  await expect(page.locator('[data-reader="early"]')).toHaveText('1.4')
  await expect(page.locator('[data-reader="late"]')).toHaveText('1.4')
})

test('title readers observe updates before subscription and share current state on remount', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
  await page.goto('/titleStyle.html')
  await expect(page.locator('[data-reader="early"]')).toHaveText('1.2')
  await page.getByRole('button', { name: 'Toggle reader' }).click()
  await expect(page.locator('[data-reader="late"]')).toHaveText('1.2')
  await page.getByRole('button', { name: 'Change size' }).click()
  await expect(page.locator('[data-reader="early"]')).toHaveText('1.4')
  await expect(page.locator('[data-reader="late"]')).toHaveText('1.4')
  await page.getByRole('button', { name: 'Toggle reader' }).click()
  await page.getByRole('button', { name: 'Toggle reader' }).click()
  await expect(page.locator('[data-reader="late"]')).toHaveText('1.4')
})

test('late initial configuration fills untouched fields without reverting edits', async ({ page }) => {
  const held = Promise.withResolvers<void>()
  await page.route('**/api/config/ui', async route => {
    await held.promise
    await route.fulfill({ json: { title_font_size: 0.8, title_color: 'accent' } })
  })
  await page.goto('/titleStyle.html')
  await page.getByRole('button', { name: 'Change size' }).click()
  held.resolve()
  await expect(page.locator('[data-color]')).toHaveText('accent')
  await expect(page.locator('[data-reader="early"]')).toHaveText('1.4')
})

for (const firstToFinish of ['A', 'B']) {
  test(`the last font choice wins when font ${firstToFinish} finishes first`, async ({ page }) => {
    await page.addInitScript(() => {
      const pending = new Map<string, () => void>()
      Object.assign(window, { finishFont: (name: string) => pending.get(name)?.() })
      Object.defineProperty(document.fonts, 'load', { value: (font: string) => {
        if (font.includes('Codystar') || font.includes('Henny Penny')) {
          return new Promise(resolve => pending.set(font.includes('Codystar') ? 'A' : 'B', () => resolve([])))
        }
        return Promise.resolve([])
      } })
    })
    await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
    const saves: { title_font?: string }[] = []
    await page.route('**/api/config/dashboard', route => {
      saves.push(route.request().postDataJSON())
      return route.fulfill({ json: { success: true } })
    })
    await page.goto('/titleStyle.html')
    await page.getByRole('button', { name: 'Font A', exact: true }).click()
    await page.getByRole('button', { name: 'Font B', exact: true }).click()
    await page.evaluate(name => (window as any).finishFont(name), firstToFinish)
    await expect(page.locator('[data-loading]')).toHaveText(firstToFinish === 'A' ? 'true' : 'false')
    await page.evaluate(name => (window as any).finishFont(name), firstToFinish === 'A' ? 'B' : 'A')
    await expect(page.locator('[data-font]')).toHaveText('henny-penny')
    await expect(page.locator('[data-loading]')).toHaveText('false')
    // Earlier edits may merge into the same save; only the font choice is under test.
    await expect.poll(() => saves.map(save => save.title_font)).toEqual(['henny-penny'])
  })
}

test('the lazy writer merges independent style fields into one authenticated save', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
  const saves: { body: unknown, token: string | undefined }[] = []
  await page.route('**/api/config/dashboard', route => {
    saves.push({ body: route.request().postDataJSON(), token: route.request().headers()['x-csrf-token'] })
    return route.fulfill({ json: { success: true } })
  })
  await page.goto('/titleStyle.html')
  await page.getByRole('button', { name: 'Save size and color' }).click()
  await expect(page.locator('[data-reader="early"]')).toHaveText('0.8')
  await expect(page.locator('[data-color]')).toHaveText('accent')
  await expect.poll(() => saves).toEqual([{ body: { title_font_size: 0.8, title_color: 'accent' }, token: CSRF_TOKEN }])
})

test('slow saves serialize while later edits merge into the next request', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
  const held = Promise.withResolvers<void>()
  const saves: unknown[] = []
  await page.route('**/api/config/dashboard', async route => {
    saves.push(route.request().postDataJSON())
    if (saves.length === 1) await held.promise
    await route.fulfill({ json: { success: true } })
  })
  await page.goto('/titleStyle.html')
  await page.getByRole('button', { name: 'Save large' }).click()
  await expect.poll(() => saves.length).toBe(1)
  await page.getByRole('button', { name: 'Save size and color' }).click()
  await page.waitForTimeout(650)
  expect(saves).toEqual([{ title_font_size: 1.4 }])
  await page.getByRole('button', { name: 'Save large' }).click()
  await page.waitForTimeout(650)
  expect(saves).toHaveLength(1)
  held.resolve()
  await expect.poll(() => saves).toEqual([{ title_font_size: 1.4 }, { title_font_size: 1.4, title_color: 'accent' }])
})

test('an identity change cancels pending saves and the next identity gets its own queue', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
  const saves: unknown[] = []
  await page.route('**/api/config/dashboard', route => {
    saves.push(route.request().postDataJSON())
    return route.fulfill({ json: { success: true } })
  })
  await page.goto('/titleStyle.html')
  await page.getByRole('button', { name: 'Save size and color' }).click()
  await page.getByRole('button', { name: 'Change subject' }).click()
  await page.waitForTimeout(650)
  expect(saves).toEqual([])
  await page.getByRole('button', { name: 'Save large' }).click()
  await expect.poll(() => saves).toEqual([{ title_font_size: 1.4 }])
})

test('title and widget appearance share one merged save with a complete theme snapshot', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
  const saves: unknown[] = []
  await page.route('**/api/config/dashboard', route => {
    saves.push(route.request().postDataJSON())
    return route.fulfill({ json: { success: true } })
  })
  await page.goto('/titleStyle.html')
  await page.getByRole('button', { name: 'Save size and color' }).click()
  await page.getByRole('button', { name: 'Save widget theme' }).click()
  await expect.poll(() => saves).toEqual([{
    title_font_size: 0.8,
    title_color: 'accent',
    widget_theme: JSON.stringify({ surface: 'outline', glow: 'none' }),
  }])
  await expect(page.locator('html')).toHaveAttribute('data-surface', 'outline')
  await expect(page.locator('html')).toHaveAttribute('data-glow', 'none')
})
