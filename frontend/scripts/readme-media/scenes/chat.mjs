// Talking to her: long-press to open the panel, Tab to Chat, ask, and watch the reply and her mouth.
import { CHAT } from '../content.mjs'
import { addCursor, APP, capture, saveFrames, settle, timeline, typing } from '../render.mjs'
import { L, start, TAG } from '../session.mjs'
import { chatEvents, chatSendAt, chatStreamMs, installStream, TYPE_AT } from '../stream.mjs'

const { browser, ctx, page } = await start()
await installStream(page, '/api/agent/process/stream', chatEvents(CHAT[L].reply))
await addCursor(ctx)
await page.goto(`${APP}/`, { waitUntil: 'domcontentloaded' })
await settle(page, 6000)
await page.mouse.move(1000, 260)
const sendAt = chatSendAt(CHAT[L].ask)
const run = timeline(page, [
  { at: 200, ms: 500, glide: [1060, 300] },
  // A long press on empty space opens the panel.
  { at: 800, run: () => page.mouse.down() }, { at: 1500, run: () => page.mouse.up() },
  { at: 2600, run: () => page.keyboard.press('Tab') },
  ...typing(page, TYPE_AT, CHAT[L].ask),
  { at: sendAt, run: () => page.keyboard.press('Enter') },
])
const ms = sendAt + chatStreamMs(CHAT[L].reply) + 2500
saveFrames(`chat-${TAG}`, await capture(page, { ms, fps: 10, script: run }))
await browser.close()
