import { expect, test } from '@playwright/test'

const platforms = [
  { id: 'custom_qq', name: 'QQ', iconName: 'SiQq' },
  { id: 'custom_telegram', name: 'Telegram', iconName: 'SiTelegram' },
  { id: 'custom_unknown', name: 'Unknown', iconName: 'MissingIcon' },
  { id: 'custom_url', name: 'Image', iconUrl: '/favicon.svg' },
].map(platform => ({ ...platform, username: 'fixture', linkType: 'url', linkPattern: 'https://example.com/{username}', color: '#00a1d6', darkColor: '#00a1d6' }))

test('late custom icons replace placeholders in mounted widgets and settings, and survive remount', async ({ page }) => {
  const held = Promise.withResolvers<void>()
  const requested = Promise.withResolvers<void>()
  await page.route('**/src/lib/iconLookup.ts*', async route => {
    requested.resolve()
    await held.promise
    await route.continue()
  })
  await page.route('**/api/config/ui', route => route.fulfill({ json: { custom_platforms: JSON.stringify(platforms) } }))
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/socialNetworkIcons.html')
  await requested.promise
  const qqIcon = page.locator('[data-platform="custom_qq"] .text-3xl svg')
  const telegramIcon = page.locator('[data-platform="custom_telegram"] .text-3xl svg')
  const unknownIcon = page.locator('[data-platform="custom_unknown"] .text-3xl svg')
  const presetIcon = page.locator('[data-platform="github"] .text-3xl svg')
  await expect(qqIcon).toHaveAttribute('viewBox', '0 0 24 24')
  await expect(telegramIcon).toHaveAttribute('viewBox', '0 0 24 24')
  const placeholder = await qqIcon.innerHTML()
  const preset = await presetIcon.innerHTML()
  await expect(page.locator('[data-platform="custom_url"] .text-3xl img')).toHaveAttribute('src', '/favicon.svg')
  await page.locator('[data-platform="custom_qq"] .widget-longpress-hint').click()
  const qqOption = page.getByRole('button').filter({ has: page.getByText('QQ', { exact: true }) })
  const telegramOption = page.getByRole('button').filter({ has: page.getByText('Telegram', { exact: true }) })
  await expect(qqOption.locator('svg').first()).toHaveAttribute('viewBox', '0 0 24 24')
  held.resolve()
  await expect.poll(() => qqIcon.innerHTML()).not.toBe(placeholder)
  await expect.poll(() => telegramIcon.innerHTML()).not.toBe(placeholder)
  const qq = await qqIcon.innerHTML()
  const telegram = await telegramIcon.innerHTML()
  expect(qq).not.toBe(telegram)
  await expect.poll(() => qqOption.locator('svg').first().innerHTML()).toBe(qq)
  await expect.poll(() => telegramOption.locator('svg').first().innerHTML()).toBe(telegram)
  expect(await unknownIcon.innerHTML()).toBe(placeholder)
  expect(await presetIcon.innerHTML()).toBe(preset)
  await qqOption.click()
  await page.getByRole('button', { name: 'Remount widgets' }).click()
  expect(await qqIcon.innerHTML()).toBe(qq)
  expect(await telegramIcon.innerHTML()).toBe(telegram)
})

test('deleting a custom platform updates mounted widgets only after a successful save', async ({ page }) => {
  await page.route('**/api/config/ui', route => route.fulfill({ json: { custom_platforms: JSON.stringify(platforms) } }))
  await page.route('**/api/csrf-token', route => route.fulfill({ json: { csrf_token: null } }))
  let failSave = true
  let saves = 0
  await page.route('**/api/config/dashboard', route => {
    saves++
    return route.fulfill({ status: failSave ? 400 : 200, json: failSave ? { error: 'Save failed' } : {} })
  })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/socialNetworkIcons.html')
  const qqIcon = page.locator('[data-platform="custom_qq"] .text-3xl svg')
  const fallback = await page.locator('[data-platform="bilibili"] .text-3xl svg').innerHTML()
  await expect(qqIcon).toHaveAttribute('viewBox', '0 0 24 24')
  await expect.poll(async () => {
    const markup = await qqIcon.innerHTML()
    return markup !== fallback && !markup.includes('M12 2C6.48')
  }).toBe(true)
  const qq = await qqIcon.innerHTML()
  await page.locator('[data-platform="custom_qq"] .widget-longpress-hint').click()
  const option = page.getByRole('button').filter({ has: page.getByText('QQ', { exact: true }) })
  const failed = page.waitForEvent('console', { predicate: message => message.text().includes('Failed to persist custom platforms') })
  page.once('dialog', dialog => dialog.accept())
  await option.locator('[role="button"]').click()
  await failed
  expect(await qqIcon.innerHTML()).toBe(qq)
  await expect(option).toBeVisible()
  failSave = false
  page.once('dialog', dialog => dialog.accept())
  await option.locator('[role="button"]').click()
  await expect(option).toHaveCount(0)
  await expect.poll(() => qqIcon.innerHTML()).toBe(fallback)
  expect(saves).toBe(2)
})

test('reopening settings retries a failed initial configuration and updates every mounted widget', async ({ page }) => {
  let fail = true
  let reads = 0
  await page.route('**/api/config/ui', route => {
    reads++
    return route.fulfill({ status: fail ? 400 : 200, json: fail ? { error: 'Read failed' } : { custom_platforms: JSON.stringify(platforms) } })
  })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const failed = page.waitForEvent('console', { predicate: message => message.text().includes('Failed to load custom platforms from API') })
  await page.goto('/socialNetworkIcons.html')
  await failed
  const qqIcon = page.locator('[data-platform="custom_qq"] .text-3xl svg')
  const telegramIcon = page.locator('[data-platform="custom_telegram"] .text-3xl svg')
  const fallback = await qqIcon.innerHTML()
  expect(await telegramIcon.innerHTML()).toBe(fallback)
  expect(reads).toBe(1)
  fail = false
  await page.locator('[data-platform="custom_qq"] .widget-longpress-hint').click()
  await expect(page.getByText('QQ', { exact: true })).toBeVisible()
  await expect.poll(() => qqIcon.innerHTML()).not.toBe(fallback)
  await expect.poll(() => telegramIcon.innerHTML()).not.toBe(fallback)
  expect(reads).toBe(2)
})
