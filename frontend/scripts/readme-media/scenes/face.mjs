// The 2.5D face in the workbench: click through expressions on the Motion tab.
import { addCursor, APP, capture, saveFrames, settle, timeline } from '../render.mjs'
import { L, start, TAG } from '../session.mjs'

const TAB = { zh: '动作调整', ja: 'モーション', en: 'Motion' }[L]
const SEQ = {
  zh: ['害羞', '兴奋', '心动过载', '哭', '>< 挤眼', '晕', '哼歌', '闭眼笑'],
  ja: ['照れ', 'わくわく', 'ときめき過ぎ', '泣く', '>< ぎゅっと目', 'くらくら', '鼻歌', '目を閉じて笑顔'],
  en: ['Shy', 'Excited', 'Lovestruck', 'Crying', '>< Squeezed eyes', 'Dizzy', 'Humming', 'Content'],
}[L]
const { browser, ctx, page } = await start()
await addCursor(ctx)
await page.goto(`${APP}/agent/settings?page=merope`, { waitUntil: 'domcontentloaded' })
await settle(page, 7000)
await page.getByRole('tab', { name: TAB }).click()
await settle(page, 2500)
const pos = []
for (const name of SEQ) {
  const r = await page.getByRole('button', { name, exact: true }).first().boundingBox()
  if (!r) throw new Error(`no expression button: ${name}`)
  pos.push([r.x + r.width / 2, r.y + r.height / 2])
}
await page.mouse.move(pos[0][0] - 120, pos[0][1] + 160)
const steps = []
let at = 400
for (const [x, y] of pos) {
  steps.push({ at, ms: 450, glide: [x, y] }, { at: at + 520, run: () => page.mouse.click(x, y) })
  at += 2000
}
saveFrames(`face-${TAG}`, await capture(page, { ms: at + 600, fps: 12, script: timeline(page, steps) }))
await browser.close()
