import type { Page } from '@playwright/test'
import { expect, test } from '@playwright/test'

const art = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="200"><rect width="100" height="200" fill="#6b5ce7"/></svg>')}`

test.use({ reducedMotion: 'reduce' })

async function mount(page: Page, query = '', count = 6) {
  await page.addInitScript(() => {
    Object.assign(window, { openedProfiles: 0 })
    window.open = () => {
      const state = window as unknown as { openedProfiles: number }
      state.openedProfiles++
      return null
    }
  })
  await page.route('**/api/**', route => route.fulfill({ json: {
    success: true,
    data: {
      platform: 'hoyolab',
      identity: { id: '800123456', name: 'Test player' },
      highlights: [],
      showcase: Array.from({ length: count }, (_, i) => ({ name: `Character ${i + 1}`, art, icon: art, level: 90, rarity: 5 })),
      profile_url: 'https://example.invalid/player',
      fetched_at: '2026-01-01T00:00:00Z',
      degraded: false,
    },
  } }))
  await page.goto(`/gamePresence.html${query}`)
  await expect(page.getByRole('button', { name: 'Character 1', exact: true })).toHaveAttribute('aria-pressed', 'true')
}

async function openedProfiles(page: Page) {
  return page.evaluate(() => (window as unknown as { openedProfiles: number }).openedProfiles)
}

test('mouse drag starting on the cover continues outside the card without opening its profile', async ({ page }) => {
  await mount(page)
  const cover = page.locator('#widget img.gp-art')
  // The artwork is enlarged and clipped; begin on the visible cover container.
  const box = (await cover.locator('../..').boundingBox())!
  await page.mouse.move(box.x + 8, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x - 60, box.y + box.height / 2)
  await page.mouse.up()
  await expect(page.getByRole('button', { name: 'Character 2', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('[data-outer-page]')).toHaveText('0')
  expect(await openedProfiles(page)).toBe(0)
  await page.mouse.move(box.x + 10, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + 70, box.y + box.height / 2, { steps: 5 })
  await page.mouse.up()
  await expect(page.getByRole('button', { name: 'Character 1', exact: true })).toHaveAttribute('aria-pressed', 'true')
  expect(await openedProfiles(page)).toBe(0)
  await cover.click()
  expect(await openedProfiles(page)).toBe(1)
})

test('dragging avatars changes character on wide and overflowing strips, while a click still selects', async ({ page }) => {
  for (const width of [400, 240]) {
    await mount(page, `?width=${width}`)
    const avatar = page.getByRole('button', { name: 'Character 1', exact: true })
    const box = (await avatar.boundingBox())!
    await page.mouse.move(box.x + 10, box.y + box.height / 2)
    await page.mouse.down()
    await page.mouse.move(box.x - 55, box.y + box.height / 2, { steps: 5 })
    await page.mouse.up()
    await expect(page.getByRole('button', { name: 'Character 2', exact: true })).toHaveAttribute('aria-pressed', 'true')
    await expect(page.locator('[data-outer-page]')).toHaveText('0')
    expect(await openedProfiles(page)).toBe(0)
    await avatar.click()
    await expect(avatar).toHaveAttribute('aria-pressed', 'true')
  }
})

test('ordinary wheel turns characters once per gesture and leaves page scrolling to the surrounding page', async ({ page }) => {
  await mount(page)
  await page.locator('#widget img.gp-art').hover()
  await page.mouse.wheel(0, 80)
  await expect(page.getByRole('button', { name: 'Character 2', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await page.mouse.wheel(0, 80)
  await expect(page.getByRole('button', { name: 'Character 2', exact: true })).toHaveAttribute('aria-pressed', 'true')
  expect(await page.evaluate(() => window.scrollY)).toBe(0)
  await page.mouse.move(600, 500)
  await page.mouse.wheel(0, 100)
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0)
  await expect(page.locator('[data-outer-page]')).toHaveText('0')
})

test('horizontal and Shift wheel work over the avatar strip without scrolling it independently', async ({ page }) => {
  await mount(page, '?width=240')
  const avatar = page.getByRole('button', { name: 'Character 1', exact: true })
  await avatar.hover()
  await page.mouse.wheel(80, 0)
  await expect(page.getByRole('button', { name: 'Character 2', exact: true })).toHaveAttribute('aria-pressed', 'true')
  // A new gesture after the real wheel sequence becomes idle.
  // Native WheelEvent.timeStamp does not follow Playwright's fake clock.
  await page.waitForTimeout(220)
  await page.getByRole('button', { name: 'Character 2', exact: true }).hover()
  await page.keyboard.down('Shift')
  await page.mouse.wheel(0, -80)
  await page.keyboard.up('Shift')
  await expect(avatar).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('[data-outer-page]')).toHaveText('0')
})

test.describe('touch input', () => {
  test.use({ hasTouch: true, viewport: { width: 375, height: 750 } })

  test('cover and avatar swipes change characters while vertical touch scrolls the page', async ({ page }) => {
    await mount(page, '?width=240')
    const input = await page.context().newCDPSession(page)
    async function swipe(x: number, y: number, dx: number, dy = 0) {
      await input.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] })
      for (const fraction of [0.3, 0.6, 1]) {
        await input.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x + dx * fraction, y: y + dy * fraction }] })
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)))
      }
      await input.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] })
    }
    const cover = (await page.locator('#widget img.gp-art').locator('../..').boundingBox())!
    await swipe(cover.x + cover.width / 2, cover.y + 80, -55)
    await expect(page.getByRole('button', { name: 'Character 2', exact: true })).toHaveAttribute('aria-pressed', 'true')
    const avatar = (await page.getByRole('button', { name: 'Character 2', exact: true }).boundingBox())!
    await swipe(avatar.x + avatar.width / 2, avatar.y + avatar.height / 2, -55)
    await expect(page.getByRole('button', { name: 'Character 3', exact: true })).toHaveAttribute('aria-pressed', 'true')
    expect(await openedProfiles(page)).toBe(0)
    await expect(page.locator('[data-outer-page]')).toHaveText('0')
    await swipe(cover.x + cover.width / 2, cover.y + 130, 0, -100)
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0)
    await expect(page.getByRole('button', { name: 'Character 3', exact: true })).toHaveAttribute('aria-pressed', 'true')
    await input.detach()
  })
})

test('editing and single-character cards leave ordinary wheel to page scrolling', async ({ page }) => {
  for (const count of [1, 6]) {
    await mount(page, '', count)
    if (count > 1) await page.getByRole('button', { name: 'Toggle editing' }).click()
    await page.locator('#widget img.gp-art').hover()
    await page.mouse.wheel(0, 100)
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(0)
    await expect(page.getByRole('button', { name: 'Character 1', exact: true })).toHaveAttribute('aria-pressed', 'true')
  }
})

test.describe('autoplay controls', () => {
  test.use({
    reducedMotion: 'no-preference',
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/144.0.0.0 Safari/537.36',
  })

  for (const width of [400, 240]) {
    test(`the avatar strip keeps stop and resume reachable at ${width}px`, async ({ page }) => {
      await page.addInitScript(() => {
        Object.defineProperties(navigator, {
          hardwareConcurrency: { get: () => 16 },
          deviceMemory: { get: () => 8 },
        })
        localStorage.setItem('animation-preference', 'standard')
      })
      await page.clock.install()
      await mount(page, `?width=${width}`)
      const toggle = page.locator('#widget button[data-rotation-ignore]')
      const widgetBox = (await page.locator('#widget').boundingBox())!
      const toggleBox = (await toggle.boundingBox())!
      expect(toggleBox.x + toggleBox.width).toBeLessThanOrEqual(widgetBox.x + widgetBox.width)
      await expect(toggle).toHaveAttribute('aria-pressed', 'false')
      const initialLabel = await toggle.getAttribute('aria-label')
      await expect(toggle).toHaveAttribute('title', initialLabel!)
      expect(toggleBox.width).toBeGreaterThanOrEqual(24)
      expect(toggleBox.height).toBeGreaterThanOrEqual(24)
      await test.info().attach('rotation-running', { body: await page.locator('#widget').screenshot(), contentType: 'image/png' })
      await toggle.click()
      await expect(toggle).toHaveAttribute('aria-pressed', 'true')
      await expect(toggle).not.toHaveAttribute('aria-label', initialLabel!)
      await expect(toggle).toHaveAttribute('title', (await toggle.getAttribute('aria-label'))!)
      await page.mouse.move(600, 500)
      await page.clock.runFor(8000)
      await expect(page.getByRole('button', { name: 'Character 1', exact: true })).toHaveAttribute('aria-pressed', 'true')
      await test.info().attach('rotation-paused', { body: await page.locator('#widget').screenshot(), contentType: 'image/png' })
      // Selecting the last avatar scrolls the strip without moving its control.
      await page.getByRole('button', { name: 'Character 6', exact: true }).click()
      expect(await toggle.boundingBox()).toEqual(toggleBox)
      await page.mouse.move(600, 500)
      await page.clock.runFor(16000)
      await expect(page.getByRole('button', { name: 'Character 6', exact: true })).toHaveAttribute('aria-pressed', 'true')
      await toggle.click()
      await expect(toggle).toHaveAttribute('aria-pressed', 'false')
      await expect(toggle).toHaveAttribute('aria-label', initialLabel!)
      await page.mouse.move(600, 500)
      await page.clock.runFor(4000)
      await expect(page.getByRole('button', { name: 'Character 1', exact: true })).toHaveAttribute('aria-pressed', 'true')
      expect(await openedProfiles(page)).toBe(0)
    })
  }
})
