// Writing in the studio editor, publishing, and the static blog showing the new post.
// Run blog-prep.mjs for the same language first.
import process from 'node:process'
import { BLOG } from '../content.mjs'
import { addCursor, APP, BACKEND, capture, saveFrames, settle, step, timeline, typing } from '../render.mjs'
import { L, ownerToken, start, TAG } from '../session.mjs'

const PUBLISH = { zh: '发布', ja: '公開', en: 'Publish' }[L]
// The editor syncs over a socket; let it through to the instance.
const appWs = `${APP.replace(/^http/, 'ws')}/`
const backendWs = `${BACKEND.replace(/^http/, 'ws')}/`
process.env.WS_PASS ||= `^${backendWs.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`
const { browser, ctx, page } = await start()
await page.addInitScript(([from, to]) => {
  const W = window.WebSocket
  const P = function (url, protocols) { return new W(String(url).replace(from, to), protocols) }
  P.prototype = W.prototype
  Object.assign(P, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 })
  window.WebSocket = P
}, [appWs, backendWs])
await addCursor(ctx)
await page.goto(`${APP}/blog/?studio=editor-new`, { waitUntil: 'domcontentloaded' })
await settle(page, 6000)
// The socket handshake needs real time.
for (let i = 0; i < 20; i++) { await page.waitForTimeout(150); await step(page, 50) }
const fr = page.frames().find((f) => /embed\/studio\/tool\/editor/.test(f.url()))
if (!fr) throw new Error('editor frame not found')
async function center(loc) { const b = await loc.boundingBox(); return b && { x: b.x + b.width / 2, y: b.y + b.height / 2, b } }
const title = await center(fr.getByPlaceholder(/标题|タイトル|Title/).first())
const pub = await center(fr.getByRole('button', { name: PUBLISH, exact: true }).first())
if (!title || !pub) throw new Error('title field or Publish button not found')
const d = BLOG[L].draft
const tMs = 70 * Array.from(d.title).length
const bMs = 45 * Array.from(d.body).length
const tx = title.x - title.b.width / 2 + 40
const run = timeline(page, [
  { at: 300, ms: 600, glide: [tx, title.y] }, { at: 1000, run: () => page.mouse.click(tx, title.y) },
  ...typing(page, 1300, d.title),
  { at: 1500 + tMs, run: () => page.keyboard.press('Enter') },
  ...typing(page, 1800 + tMs, d.body, 45),
  { at: 3000 + tMs + bMs, ms: 700, glide: [pub.x, pub.y] }, { at: 3900 + tMs + bMs, run: () => page.mouse.click(pub.x, pub.y) },
])
const recA = await capture(page, { ms: 5500 + tMs + bMs, fps: 10, script: run })

// Publishing rebuilds the static site; wait for it in real time.
const token = await ownerToken()
const blogHtml = async () => (await fetch(`${BACKEND}/blog/`, { headers: { authorization: `Bearer ${token}` } })).text()
for (let i = 0; i < 60 && !(await blogHtml()).includes(d.title); i++) await new Promise((r) => setTimeout(r, 1000))
await page.goto(`${APP}/blog/`, { waitUntil: 'domcontentloaded' })
await settle(page, 2500)
const link = await page.getByRole('link', { name: d.title }).first().boundingBox()
if (!link) throw new Error('new post is not on the blog index')
const lx = link.x + Math.min(60, link.width / 2)
const ly = link.y + link.height / 2
await page.mouse.move(lx + 260, ly + 220)
const runB = timeline(page, [
  { at: 1400, ms: 700, from: [lx + 260, ly + 220], glide: [lx, ly] }, { at: 2300, run: () => page.mouse.click(lx, ly) },
])
const recB = await capture(page, { ms: 6500, fps: 10, script: runB })
saveFrames(`blog-${TAG}`, { frames: [...recA.frames, ...recB.frames], dt: recA.dt })
await browser.close()
