import type { Page } from '@playwright/test'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const fixture = `/@fs${fileURLToPath(new URL('./fixture/tappPopup.tsx', import.meta.url))}`

async function mountFixture(page: Page, onIssue: () => number = () => 1, onRevoke: () => void = () => {}) {
  await page.route('**/tapp-popup-probe', route => route.fulfill({ contentType: 'text/html', body: '<style>.w-full{width:100%}.h-full{height:100%}.relative{position:relative}.flex-1{flex:1 1 0%}.min-h-0{min-height:0}</style><div id="root"></div>' }))
  await page.route('**/api/**', async route => {
    const request = route.request()
    if (request.url().includes('runtime-grants') && request.method() === 'DELETE') {
      onRevoke()
      return route.fulfill({ json: {} })
    }
    if (request.url().endsWith('/runtime-grants')) {
      const n = onIssue()
      const { instanceId, kind } = request.postDataJSON()
      return route.fulfill({ json: { version: 2, token: `fixture-token-${n}`, runtimeId: `fixture-runtime-${n}`, tappId: 'fixture.popup', ownerId: 1, subjectId: 0, instanceId, kind, permissions: [], expiresAt: new Date(Date.now() + 3600_000).toISOString() } })
    }
    return route.fulfill({ json: request.url().includes('csrf-token') ? { csrf_token: null } : {} })
  })
  await page.goto('/tapp-popup-probe')
  await page.evaluate(async fixture => { (await import(fixture)).mount() }, fixture)
}

/**
 * Wait for the popup frame to land. Clicks into its cross-origin iframe are aimed from the
 * iframe's own layout, which does not move while the outer frame springs into place.
 */
async function settled(page: Page) {
  await page.locator('.widget-popup-frame').evaluate(node => Promise.all(node.getAnimations().map(animation => animation.finished)))
}

test('TAPP popup layer opens only on a gesture, retracts without destroying, and dies with the app', async ({ page }) => {
  test.setTimeout(90_000)
  let issued = 0
  let revoked = 0
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  await mountFixture(page, () => ++issued, () => revoked++)
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
  await settled(page)
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
  // Opened by a click, so focus is not handed back: not into the card's iframe (that would
  // look like a user gesture to the app), and not onto the card as a stray focus ring.
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('BODY')

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
  await settled(page)
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

test('on a phone the popup is a sheet that follows a drag on its handle and is flicked away', async ({ page }) => {
  test.setTimeout(60_000)
  await page.setViewportSize({ width: 390, height: 844 })
  await mountFixture(page)
  const frame = page.locator('.widget-popup-frame')
  const card = page.frameLocator('iframe.tapp-widget-iframe').first()
  await expect.poll(() => card.locator('body').getAttribute('data-boot-open')).not.toBeNull()
  await card.getByRole('button', { name: 'Open popup' }).click()
  await expect(frame).toHaveAttribute('data-phase', 'open')
  await expect(frame).toHaveClass(/widget-popup-frame--sheet/)
  await settled(page)
  const handle = (await page.locator('.widget-popup-frame__grabber').boundingBox())!
  const x = handle.x + handle.width / 2
  const y = handle.y + handle.height / 2

  // A short, slow drag follows the finger and springs back.
  await page.mouse.move(x, y)
  await page.mouse.down()
  for (let step = 1; step <= 8; step++) {
    await page.mouse.move(x, y + step * 5)
    await page.waitForTimeout(30)
  }
  expect(await frame.evaluate(node => node.style.transform)).toBe('translateY(40px)')
  await page.waitForTimeout(100)
  await page.mouse.up()
  expect(await frame.evaluate(node => node.style.transform)).toBe('')
  await settled(page)
  await expect(frame).toHaveAttribute('data-phase', 'open')

  // Past a quarter of its height it lets go and retracts.
  await page.mouse.move(x, y)
  await page.mouse.down()
  for (let step = 1; step <= 10; step++) {
    await page.mouse.move(x, y + step * 30)
    await page.waitForTimeout(30)
  }
  await page.waitForTimeout(100)
  await page.mouse.up()
  await expect(frame).toHaveAttribute('data-phase', 'stashed')
})
