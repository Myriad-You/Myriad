// A report card's stage: click Play on the Steam card and let the acts play.
import { addCursor, APP, capture, saveFrames, settle, timeline } from '../render.mjs'
import { L, start, TAG } from '../session.mjs'

const PLAY = { zh: '播放报告', ja: 'レポートを再生', en: 'Play report' }[L]
const { browser, ctx, page } = await start()
await addCursor(ctx)
await page.goto(`${APP}/`, { waitUntil: 'domcontentloaded' })
await settle(page, 6000)
await page.mouse.move(900, 330)
// The top-left card with a Play button is Steam in the sample layout.
const boxes = await page.getByRole('button', { name: PLAY }).evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2, r.width] }))
const [bx, by] = boxes.filter((b) => b[2] > 0).sort((a, b) => a[1] - b[1] || a[0] - b[0])[0]
const run = timeline(page, [
  { at: 300, ms: 800, glide: [bx, by] },
  { at: 1200, run: () => page.mouse.click(bx, by) },
  { at: 1700, ms: 500, glide: [1160, 760] },
])
saveFrames(`report-${TAG}`, await capture(page, { ms: 17000, fps: 10, script: run }))
await browser.close()
