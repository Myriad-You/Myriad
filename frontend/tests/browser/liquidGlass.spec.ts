import type { Page } from '@playwright/test'
import { expect, test } from '@playwright/test'
import sharp from 'sharp'

async function lens(page: Page, id: string) {
  return page.locator(`#${id}`).evaluate((el) => {
    const ref = (el as HTMLElement).style.getPropertyValue('--hyalite')
    const filter = document.getElementById(ref.slice(5, -1))
    return {
      ref, exists: !!filter,
      width: filter?.getAttribute('width'), height: filter?.getAttribute('height'),
      map: filter?.querySelector('feImage')?.getAttribute('href'),
      images: Iterator.from(filter?.querySelectorAll('feImage') ?? []).toArray().map(n => n.getAttribute('width')),
      tone: filter?.querySelector('[result="toned"] feFuncR')?.getAttribute('tableValues'),
      computed: getComputedStyle(el).backdropFilter,
    }
  })
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 1000 })
  await page.goto('/liquidGlass.html')
  await expect(page.locator('#card')).toHaveAttribute('data-liquid-lens', 'surface')
  await expect(page.locator('#island')).toHaveAttribute('data-liquid-lens', 'chrome')
  await page.evaluate(async () => {
    await document.fonts.ready
    await Promise.all(document.getAnimations().map(animation => animation.finished.catch(() => {})))
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
  })
})

test('different outlines have independent maps, twins share a filter, descendants do not inherit filtering', async ({ page }) => {
  const card = await lens(page, 'card')
  expect(card.exists).toBe(true)
  expect(await page.locator('#dialog').evaluate(el => getComputedStyle(el).getPropertyValue('--surface-tint'))).toContain('radial-gradient')
  expect(card.width).toBe('300')
  expect((await lens(page, 'twin')).ref).toBe(card.ref)
  expect((await lens(page, 'local')).map).not.toBe(card.map)
  expect(await page.locator('#plain-child').evaluate(el => getComputedStyle(el).backdropFilter)).toBe('none')
  await expect(page.locator('#agent')).not.toHaveAttribute('data-liquid-lens')
})

test('theme switches preserve forced local liquid and dark tuning reuses geometry', async ({ page }) => {
  const map = (await lens(page, 'card')).map
  const builds = await page.evaluate(() => window.__liquid.builds())
  await page.evaluate(() => document.documentElement.classList.add('dark'))
  await expect.poll(async () => (await lens(page, 'card')).tone).toBe('0 0.16 0.3 0.42 0.5')
  expect((await lens(page, 'dialog')).tone).toBe('0 0.22 0.44 0.65 0.78')
  expect((await lens(page, 'card')).map).toBe(map)
  expect(await page.evaluate(() => window.__liquid.builds())).toBe(builds)
  await page.evaluate(() => { document.documentElement.dataset.surface = 'solid' })
  await expect(page.locator('#card')).not.toHaveAttribute('data-liquid-lens')
  await expect(page.locator('#local')).toHaveAttribute('data-liquid-lens', 'surface')
  expect((await lens(page, 'card')).computed).toBe('none')
  await page.evaluate(() => document.getElementById('local')!.classList.remove('glass-liquid'))
  await expect(page.locator('#local')).not.toHaveAttribute('data-liquid-lens')
})

test('portals attach, DOM moves retain the lens, and removal frees its filter', async ({ page }) => {
  await page.evaluate(() => {
    const el = document.createElement('div')
    el.id = 'portal'
    el.className = 'glass'
    el.style.cssText = 'position:fixed;right:20px;top:20px;width:177px;height:83px;border-radius:17px'
    document.body.append(el)
  })
  await expect(page.locator('#portal')).toHaveAttribute('data-liquid-lens')
  const ref = (await lens(page, 'portal')).ref
  await page.evaluate(() => document.querySelector('main')!.append(document.getElementById('portal')!))
  expect((await lens(page, 'portal')).ref).toBe(ref)
  await page.locator('#portal').evaluate(el => el.remove())
  await expect.poll(() => page.evaluate(id => !!document.getElementById(id), ref.slice(5, -1))).toBe(false)
})

test('resizing stretches a private filter without clipping or dropping to blur, then updates radii', async ({ page }) => {
  const twin = await lens(page, 'twin')
  await page.locator('#card').evaluate((el) => {
    (el as HTMLElement).style.width = '420px'
    ;(el as HTMLElement).style.borderRadius = '40px'
  })
  await expect.poll(async () => (await lens(page, 'card')).width).toBe('420')
  const resized = await lens(page, 'card')
  expect(resized.ref).toMatch(/^url\(#/)
  expect(resized.images.every(w => w === '420')).toBe(true)
  expect((await lens(page, 'twin')).ref).toBe(twin.ref)
  await expect.poll(async () => (await lens(page, 'card')).ref).not.toContain('-resize')
  const before = (await lens(page, 'card')).map
  await page.locator('#card').evaluate((el) => {
    (el as HTMLElement).style.borderRadius = '8px'
    el.dispatchEvent(new TransitionEvent('transitionend', { bubbles: true, propertyName: 'border-radius' }))
  })
  await expect.poll(async () => (await lens(page, 'card')).map).not.toBe(before)
})

test('control island keeps refraction throughout the production width and radius transition', async ({ page }) => {
  await page.locator('#island').evaluate(el => el.classList.add('expanded', 'gcp-animating'))
  const samples = await page.locator('#island').evaluate(el => new Promise<string[]>((resolve) => {
    const refs: string[] = []
    const start = performance.now()
    const tick = () => {
      refs.push((el as HTMLElement).style.getPropertyValue('--hyalite'))
      if (performance.now() - start < 850) requestAnimationFrame(tick)
      else resolve(refs)
    }
    requestAnimationFrame(tick)
  }))
  expect(samples.length).toBeGreaterThan(5)
  expect(samples.every(ref => ref.startsWith('url(#'))).toBe(true)
  await page.locator('#island').evaluate(el => el.classList.remove('gcp-animating'))
  await expect.poll(async () => (await lens(page, 'island')).width).toBe('400')
})

test('mobile, performance and reduced-motion gates stop map generation and restore cleanly', async ({ page }) => {
  for (const mode of ['light', 'exlight']) {
    await page.evaluate(mode => { document.documentElement.dataset.perfMode = mode }, mode)
    await expect(page.locator('[data-liquid-lens]')).toHaveCount(0)
    if (mode === 'exlight') expect((await lens(page, 'card')).computed).toBe('none')
  }
  const builds = await page.evaluate(() => window.__liquid.builds())
  await page.locator('#card').evaluate(el => (el as HTMLElement).style.width = '350px')
  await page.setViewportSize({ width: 600, height: 1000 })
  await page.evaluate(() => { document.documentElement.dataset.perfMode = 'standard' })
  await expect(page.locator('[data-liquid-lens]')).toHaveCount(0)
  expect(await page.evaluate(() => window.__liquid.builds())).toBe(builds)
  await page.setViewportSize({ width: 1200, height: 1000 })
  await expect(page.locator('#card')).toHaveAttribute('data-liquid-lens')
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await expect(page.locator('[data-liquid-lens]')).toHaveCount(0)
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  await expect(page.locator('#card')).toHaveAttribute('data-liquid-lens')
})

test('offscreen surfaces release resources and restore on entry', async ({ page }) => {
  await expect(page.locator('#offscreen')).not.toHaveAttribute('data-liquid-lens')
  await page.locator('#offscreen').scrollIntoViewIfNeeded()
  await expect(page.locator('#offscreen')).toHaveAttribute('data-liquid-lens')
  await expect(page.locator('#card')).not.toHaveAttribute('data-liquid-lens')
  await page.locator('#card').scrollIntoViewIfNeeded()
  await expect(page.locator('#card')).toHaveAttribute('data-liquid-lens')
})

test('unsupported engines retain CSS fallback and disposal supports remount without dangling URLs', async ({ page }) => {
  await page.evaluate(() => { window.__liquid.engine.force(false); window.__liquid.start() })
  await expect(page.locator('[data-liquid-lens]')).toHaveCount(0)
  expect((await lens(page, 'card')).computed).toContain('blur(')
  expect(await page.locator('filter[id^="myriad-lens-"]').count()).toBe(0)
  await page.evaluate(() => { window.__liquid.engine.force(null); window.__liquid.start() })
  await expect(page.locator('#card')).toHaveAttribute('data-liquid-lens')
  expect((await lens(page, 'card')).exists).toBe(true)
  await page.evaluate(() => window.__liquid.stop())
  await expect(page.locator('[data-liquid-lens]')).toHaveCount(0)
  expect(await page.locator('filter[id^="myriad-lens-"]').count()).toBe(0)
})

test('materialization changes the rim alpha, leaves the ring mask intact, and disposes transient filters immediately', async ({ page }) => {
  const state = await page.evaluate(() => {
    const el = document.getElementById('probe')!
    window.__liquid.engine.attach(el, { materialize: 1000, smooth: 1, rim: 0.5 })
    const ref = el.style.getPropertyValue('--hyalite')
    const filter = document.getElementById(ref.slice(5, -1))!
    const result = {
      rim: filter.querySelector('[result="rimLit"] feFuncA')!.getAttribute('slope'),
      ring: filter.querySelector('[result="ringInv"] feFuncA')!.getAttribute('tableValues'),
      ringSlope: filter.querySelector('[result="ringInv"] feFuncA')!.getAttribute('slope'),
    }
    window.__liquid.engine.detach(el)
    return { ...result, removed: !document.getElementById(ref.slice(5, -1)) }
  })
  expect(state).toEqual({ rim: '0', ring: '1 0', ringSlope: null, removed: true })
})

test('the browser actually bends backdrop pixels and renders both themes', async ({ page }, testInfo) => {
  const probe = page.locator('#probe')
  const before = await probe.screenshot()
  await page.evaluate(() => window.__liquid.engine.attach(document.getElementById('probe')!, {
    blur: 0, rim: 0, smooth: 0, dispersion: 0,
  }))
  const after = await probe.screenshot()
  const a = await sharp(before).removeAlpha().raw().toBuffer()
  const b = await sharp(after).removeAlpha().raw().toBuffer()
  let changed = 0
  for (let i = 0; i < a.length; i += 3) {
    if (Math.abs(a[i] - b[i]) + Math.abs(a[i + 1] - b[i + 1]) + Math.abs(a[i + 2] - b[i + 2]) > 20) changed++
  }
  expect(changed).toBeGreaterThan(150)
  await page.evaluate(() => window.__liquid.engine.detach(document.getElementById('probe')!))
  await page.screenshot({ path: testInfo.outputPath('liquid-light.png'), animations: 'disabled' })
  await page.evaluate(() => document.documentElement.classList.add('dark'))
  await expect.poll(async () => (await lens(page, 'card')).tone).toBeTruthy()
  await page.screenshot({ path: testInfo.outputPath('liquid-dark.png'), animations: 'disabled' })
})

test('idle navigation releases its lens and resumes with unchanged geometry', async ({ page }) => {
  await page.locator('#card').evaluate(el => {
    const nav = document.createElement('div')
    nav.className = 'nav-container'
    el.before(nav)
    nav.append(el)
    nav.dataset.navIdle = 'hidden'
  })
  await expect(page.locator('#card')).not.toHaveAttribute('data-liquid-lens')
  await page.evaluate(() => { document.querySelector<HTMLElement>('.nav-container')!.dataset.navIdle = 'visible' })
  await expect(page.locator('#card')).toHaveAttribute('data-liquid-lens')
})

test('non-geometric changes leave a settled lens untouched', async ({ page }) => {
  const mutations = await page.locator('#card').evaluate(async (el) => {
    let writes = 0
    const observer = new MutationObserver(records => { writes += records.length })
    observer.observe(el, { attributes: true, attributeFilter: ['style'] })
    el.classList.add('unrelated-state')
    el.dispatchEvent(new TransitionEvent('transitionend', { bubbles: true, propertyName: 'background-color' }))
    el.dispatchEvent(new TransitionEvent('transitionend', { bubbles: true, propertyName: 'box-shadow' }))
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
    observer.disconnect()
    return writes
  })
  expect(mutations).toBe(0)
  expect((await lens(page, 'card')).exists).toBe(true)
})

test('repeated class mutations discover a surface subtree once per delivery', async ({ page }) => {
  const before = await lens(page, 'card')
  const scans = await page.locator('#card').evaluate(async (el) => {
    const original = el.querySelectorAll
    let scans = 0
    el.querySelectorAll = ((selector: string) => {
      if (selector.includes('.control-bar-trigger')) scans++
      return original.call(el, selector)
    }) as typeof el.querySelectorAll
    try {
      for (let i = 0; i < 100; i++) el.classList.toggle('unrelated-state')
      await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
      return scans
    } finally {
      el.querySelectorAll = original
    }
  })
  expect(scans).toBe(1)
  expect((await lens(page, 'card')).map).toBe(before.map)
})
