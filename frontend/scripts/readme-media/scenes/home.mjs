// The homepage: widgets settle, report-card covers rotate.
import { APP, capture, saveFrames, settle } from '../render.mjs'
import { start, TAG } from '../session.mjs'

const { browser, page } = await start()
await page.goto(`${APP}/`, { waitUntil: 'domcontentloaded' })
await settle(page, 7000)
saveFrames(`home-${TAG}`, await capture(page, { ms: 12000, fps: 10 }))
await browser.close()
