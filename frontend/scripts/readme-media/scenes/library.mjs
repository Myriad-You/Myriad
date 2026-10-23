// The library canvas: drag, zoom out, then switch categories from the side rail.
import { addCursor, APP, capture, saveFrames, settle, timeline } from '../render.mjs'
import { start, TAG } from '../session.mjs'

const { browser, ctx, page } = await start()
await addCursor(ctx)
await page.goto(`${APP}/library`, { waitUntil: 'domcontentloaded' })
await settle(page, 7000)
// Dismiss the canvas hint for this session.
await page.mouse.click(900, 29)
await settle(page, 1200)
await page.mouse.move(1220, 230)
const m = page.mouse
const zoomOut = [635, 855]
const rail = { all: [37, 317], games: [37, 373], videos: [37, 429] }
const run = timeline(page, [
  { at: 300, run: () => m.down() }, { at: 300, ms: 1600, glide: [640, 330] }, { at: 2000, run: () => m.up() },
  { at: 2500, ms: 700, glide: zoomOut }, { at: 3300, run: () => m.click(...zoomOut) }, { at: 3900, run: () => m.click(...zoomOut) }, { at: 4500, run: () => m.click(...zoomOut) },
  { at: 6200, ms: 800, glide: rail.games }, { at: 7100, run: () => m.click(...rail.games) },
  { at: 9300, ms: 600, glide: rail.videos }, { at: 10000, run: () => m.click(...rail.videos) },
  { at: 12200, ms: 600, glide: rail.all }, { at: 12900, run: () => m.click(...rail.all) },
])
saveFrames(`library-${TAG}`, await capture(page, { ms: 15000, fps: 10, script: run }))
await browser.close()
