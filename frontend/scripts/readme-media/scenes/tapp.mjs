// Tapp Playground: describe an app, watch it get built and checked, then use it in the preview.
import { PROMPT, streamEvents } from '../pomodoro.mjs'
import { addCursor, APP, capture, saveFrames, settle, timeline, typing } from '../render.mjs'
import { L, start, TAG } from '../session.mjs'
import { installStream } from '../stream.mjs'

const GO = { zh: '生成并运行', ja: '生成して実行', en: 'Generate & Run' }[L]
// Init scripts must also run inside the sandboxed preview frame.
const { browser, ctx, page } = await start({ chromeArgs: ['--disable-features=IsolateSandboxedIframes'] })
// Recording only: the preview sandbox replaces eval, which the fake clock needs in every frame.
await page.addInitScript(() => {
  // eslint-disable-next-line no-eval -- keeps the page's own eval, nothing is evaluated here
  const real = window.eval
  try { Object.defineProperty(window, 'eval', { get: () => real, set: () => {}, configurable: false }) } catch {}
})
await installStream(page, '/api/tapp-playground/generate-stream', streamEvents(L))
await addCursor(ctx)
await page.goto(`${APP}/tapp/playground`, { waitUntil: 'domcontentloaded' })
await settle(page, 6000)
// The preview-capability notice is dismissible; close it so it does not cover the panes.
const closers = await page.evaluate(() => [...document.querySelectorAll('button')]
  .map((b) => b.getBoundingClientRect())
  .filter((r) => r.width > 0 && r.width < 40 && r.y < 70 && r.x > 500 && r.x < 1100)
  .map((r) => [r.x + r.width / 2, r.y + r.height / 2]))
for (const [x, y] of closers) await page.mouse.click(x, y)
await settle(page, 800)
const box = await page.getByRole('textbox').last().boundingBox()
const go = await page.getByRole('button', { name: GO }).last().boundingBox()
if (!box || !go) throw new Error('prompt box or Generate button not found')
const [tx, ty] = [box.x + 40, box.y + box.height / 2]
const [gx, gy] = [go.x + go.width / 2, go.y + go.height / 2]
await page.mouse.move(tx + 200, ty - 200)
const typeMs = 70 * Array.from(PROMPT[L]).length
let startAt = [400, 600]
const run = timeline(page, [
  { at: 300, ms: 500, glide: [tx, ty] }, { at: 900, run: () => page.mouse.click(tx, ty) },
  ...typing(page, 1200, PROMPT[L]),
  { at: 1500 + typeMs, ms: 500, glide: [gx, gy] }, { at: 2100 + typeMs, run: () => page.mouse.click(gx, gy) },
  // By now the generated app is running in the preview; press its Start button.
  { at: 11600 + typeMs, run: async () => {
    const b = await page.frameLocator('iframe').first().locator('#pf-start').boundingBox()
    if (b) startAt = [b.x + b.width / 2, b.y + b.height / 2]
  } },
  { at: 11700 + typeMs, ms: 700, glide: () => startAt }, { at: 12500 + typeMs, run: () => page.mouse.click(...startAt) },
])
saveFrames(`tapp-${TAG}`, await capture(page, { ms: 19000 + typeMs, fps: 10, script: run }))
await browser.close()
