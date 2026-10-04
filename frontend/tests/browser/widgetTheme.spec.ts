import { expect, test } from '@playwright/test'

test('a failed theme read does not save unknown fields, and remount restores untouched server fields', async ({ page }) => {
  let fail = true
  await page.route('**/api/config/ui', route => route.fulfill({
    status: fail ? 400 : 200,
    json: fail ? { error: 'Read failed' } : { widget_theme: JSON.stringify({ surface: 'liquid', glow: 'primary' }) },
  }))
  await page.route('**/api/csrf-token', route => route.fulfill({ json: { csrf_token: null } }))
  const saves: unknown[] = []
  await page.route('**/api/config/dashboard', route => {
    saves.push(route.request().postDataJSON())
    return route.fulfill({ json: {} })
  })
  const failed = page.waitForEvent('console', { predicate: message => message.text().includes('加载小组件主题失败') })
  await page.goto('/widgetTheme.html')
  await failed
  await page.getByRole('button', { name: 'Outline', exact: true }).click()
  await expect(page.locator('[data-reader="early"]')).toHaveText('outline/identity')
  await page.waitForTimeout(650)
  expect(saves).toEqual([])
  fail = false
  await page.getByRole('button', { name: 'Toggle reader', exact: true }).click()
  await expect(page.locator('[data-reader="early"]')).toHaveText('outline/primary')
  await expect(page.locator('[data-reader="late"]')).toHaveText('outline/primary')
  await page.getByRole('button', { name: 'Outline', exact: true }).click()
  await expect.poll(() => saves).toEqual([{ widget_theme: JSON.stringify({ surface: 'outline', glow: 'primary' }) }])
})

test('widget appearance readers and root attributes stay in sync across subscription and remount', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: {} }))
  await page.goto('/widgetTheme.html')
  await expect(page.locator('[data-reader="early"]')).toHaveText('solid/identity')
  await expect(page.locator('html')).toHaveAttribute('data-surface', 'solid')
  await page.getByRole('button', { name: 'Toggle reader' }).click()
  await expect(page.locator('[data-reader="late"]')).toHaveText('solid/identity')
  await page.getByRole('button', { name: 'Outline', exact: true }).click()
  await expect(page.locator('[data-reader="early"]')).toHaveText('outline/identity')
  await expect(page.locator('[data-reader="late"]')).toHaveText('outline/identity')
  await expect(page.locator('html')).toHaveAttribute('data-surface', 'outline')
  await page.getByRole('button', { name: 'Defaults', exact: true }).click()
  await expect(page.locator('[data-reader="early"]')).toHaveText('glass/identity')
  await expect(page.locator('html')).not.toHaveAttribute('data-surface')
  await expect(page.locator('html')).not.toHaveAttribute('data-glow')
})

test('late initial theme respects edited surface but supplies untouched glow', async ({ page }) => {
  const held = Promise.withResolvers<void>()
  await page.route('**/api/config/ui', async route => {
    await held.promise
    await route.fulfill({ json: { widget_theme: JSON.stringify({ surface: 'liquid', glow: 'primary' }) } })
  })
  await page.goto('/widgetTheme.html')
  await page.getByRole('button', { name: 'Outline', exact: true }).click()
  held.resolve()
  await expect(page.locator('[data-reader="early"]')).toHaveText('outline/primary')
  await expect(page.locator('html')).toHaveAttribute('data-surface', 'outline')
  await expect(page.locator('html')).toHaveAttribute('data-glow', 'primary')
})

for (const changeIdentity of [false, true]) {
  test(`saving during initialization preserves untouched fields (identity changes=${changeIdentity})`, async ({ page }) => {
    const held = Promise.withResolvers<void>()
    await page.route('**/api/config/ui', async route => {
      await held.promise
      await route.fulfill({ json: { widget_theme: JSON.stringify({ surface: 'liquid', glow: 'primary' }) } })
    })
    const saves: unknown[] = []
    await page.route('**/api/config/dashboard', route => {
      saves.push(route.request().postDataJSON())
      return route.fulfill({ json: { success: true } })
    })
    await page.goto('/widgetTheme.html')
    await page.getByRole('button', { name: 'Save outline', exact: true }).click()
    await expect(page.locator('html')).toHaveAttribute('data-surface', 'outline')
    await page.waitForTimeout(650)
    expect(saves).toEqual([])
    if (changeIdentity) await page.getByRole('button', { name: 'Change subject' }).click()
    held.resolve()
    await expect(page.locator('[data-reader="early"]')).toHaveText('outline/primary')
    if (changeIdentity) {
      await page.waitForTimeout(650)
      expect(saves).toEqual([])
    } else {
      await expect.poll(() => saves).toEqual([{ widget_theme: JSON.stringify({ surface: 'outline', glow: 'primary' }) }])
    }
  })
}
