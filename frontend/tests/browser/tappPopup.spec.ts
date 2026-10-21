import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const fixture = `/@fs${fileURLToPath(new URL('./fixture/tappPopup.tsx', import.meta.url))}`

test('TAPP popup layer opens only on a gesture, retracts without destroying, and dies with the app', async ({ page }) => {
  test.setTimeout(90_000)
  let issued = 0
  let revoked = 0
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  await page.route('**/tapp-popup-probe', route => route.fulfill({ contentType: 'text/html', body: '<style>.w-full{width:100%}.h-full{height:100%}.relative{position:relative}.flex-1{flex:1 1 0%}.min-h-0{min-height:0}</style><div id="root"></div>' }))
  await page.route('**/api/**', async route => {
    const request = route.request()
    if (request.url().includes('runtime-grants') && request.method() === 'DELETE') {
      revoked++
      return route.fulfill({ json: {} })
    }
    if (request.url().endsWith('/runtime-grants')) {
      issued++
      const { instanceId, kind } = request.postDataJSON()
      return route.fulfill({ json: { version: 2, token: `fixture-token-${issued}`, runtimeId: `fixture-runtime-${issued}`, tappId: 'fixture.popup', ownerId: 1, subjectId: 0, instanceId, kind, permissions: [], expiresAt: new Date(Date.now() + 3600_000).toISOString() } })
    }
    return route.fulfill({ json: request.url().includes('csrf-token') ? { csrf_token: null } : {} })
  })
  await page.goto('/tapp-popup-probe')
  await page.evaluate(async fixture => { (await import(fixture)).mount() }, fixture)
  const snapshot = () => page.evaluate(async fixture => (await import(fixture)).snapshot(), fixture)
  const frame = page.locator('.widget-popup-frame')
  const card = page.frameLocator('iframe.tapp-widget-iframe').first()
  const popup = page.frameLocator('.widget-popup-frame iframe')
  const cardAttr = (name: string) => card.locator('body').getAttribute(name)
  const popupAttr = (name: string) => popup.locator('body').getAttribute(name)

  await expect(card.getByRole('button', { name: 'Open popup' })).toBeVisible()
  // The card cannot open its popup on its own at boot.
  await expect.poll(() => cardAttr('data-boot-open')).toBe('Popup must follow a user action on this widget')
  expect((await snapshot()).entries).toBe(0)

  await card.getByRole('button', { name: 'Open popup' }).click()
  await expect.poll(() => cardAttr('data-open')).toBe('ok:1')
  await expect(page.getByRole('dialog')).toBeVisible()
  await expect(popup.locator('#params')).toHaveText('{"n":1}')
  const boot = await popupAttr('data-boot')
  const popupIframe = await page.locator('.widget-popup-frame iframe').elementHandle()
  expect(await snapshot()).toMatchObject({ card: { bridges: 1, active: 1 }, popup: { bridges: 1, active: 1 }, entries: 1, activeKey: 'card' })

  // Shift+Tab out of the first control inside the cross-origin popup stays in the popup.
  await popup.getByRole('button', { name: 'Close from popup' }).focus()
  await page.keyboard.press('Shift+Tab')
  expect(await page.evaluate(() => document.activeElement?.className)).toBe('widget-popup-frame__close')

  // Close from inside the popup: retract, not destroy.
  await popup.getByRole('button', { name: 'Close from popup' }).click()
  await expect.poll(() => popupAttr('data-close')).toBe('ok')
  await expect(frame).toHaveAttribute('data-phase', 'stashed')
  await expect.poll(() => popupAttr('data-pauses')).toBe('1')
  expect(await popupIframe!.evaluate(node => node.isConnected)).toBe(true)
  expect(await snapshot()).toMatchObject({ popup: { bridges: 1, destroyed: 0, active: 0 }, entries: 1, activeKey: null })
  // Focus goes back to the card container, not into the card's iframe.
  expect(await page.evaluate(() => document.activeElement?.classList.contains('tapp-widget-sandbox'))).toBe(true)

  // A click arms a card that keeps calling openPopup. That click may open it once…
  await page.waitForTimeout(900)
  await card.getByRole('button', { name: 'Arm auto open' }).click()
  await expect.poll(() => cardAttr('data-auto-opens')).toBe('1')
  await expect(popup.locator('#params')).toHaveText('{"auto":true}')
  expect(await popupAttr('data-boot')).toBe(boot)
  // …but after the user dismisses it, the timer cannot bring it back.
  await page.keyboard.press('Escape')
  await expect(frame).toHaveAttribute('data-phase', 'stashed')
  await page.waitForTimeout(2500)
  expect(await cardAttr('data-auto-opens')).toBe('1')
  await expect(frame).toHaveAttribute('data-phase', 'stashed')

  // A real click reopens the same sandbox with new params.
  await card.getByRole('button', { name: 'Open popup' }).click()
  await expect.poll(() => cardAttr('data-open')).toBe('ok:2')
  await expect(popup.locator('#params')).toHaveText('{"n":2}')
  await expect.poll(() => popupAttr('data-resumes')).toBe('2')
  expect(await page.locator('.widget-popup-frame iframe').evaluate((node, original) => node === original, popupIframe)).toBe(true)

  // Escape pressed inside the popup's iframe retracts it too.
  await popup.locator('#params').click()
  await page.keyboard.press('Escape')
  await expect(frame).toHaveAttribute('data-phase', 'stashed')
  expect(issued).toBe(1)
  expect(revoked).toBe(0)

  // The app stops: the authorization premise is gone, so the retracted popup is destroyed.
  await page.evaluate(async fixture => (await import(fixture)).stopTapp(), fixture)
  await expect(frame).toHaveCount(0)
  await expect.poll(async () => (await snapshot()).popup.destroyed).toBe(1)
  expect((await snapshot()).entries).toBe(0)
  expect(errors).toEqual([])
})
