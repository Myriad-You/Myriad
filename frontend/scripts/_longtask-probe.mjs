import { chromium } from '@playwright/test'
import { spawn } from 'node:child_process'
import { appendFileSync, writeFileSync } from 'node:fs'
import { setTimeout as sleep } from 'node:timers/promises'

const PORT = 4199
const TOTAL_MS = 3_000_000
const SAMPLE_MS = 15_000
const origin = `http://127.0.0.1:${PORT}`
const out = '/tmp/myriad-lt-3000.jsonl'
writeFileSync(out, '')

const server = spawn('node', ['scripts/spa-server.mjs'], {
  cwd: '/Users/hitomi/GitHub/Myriad/frontend',
  env: { ...process.env, PORT: String(PORT), HOST: '127.0.0.1' },
  stdio: ['ignore', 'ignore', 'inherit'],
})

let ready = false
for (let i = 0; i < 50 && !ready; i++) {
  try {
    const res = await fetch(origin, { signal: AbortSignal.timeout(400) })
    if (res.status) ready = true
  } catch {
    await sleep(200)
  }
}
if (!ready) {
  server.kill()
  throw new Error('spa-server did not start')
}

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage()
await page.route('**/api/**', (route) => {
  const path = new URL(route.request().url()).pathname
  let data = { success: true, data: [], items: [], sources: [], installations: [] }
  if (path === '/api/setup/status') data = { is_setup_required: false }
  if (path === '/api/auth/me') data = { authenticated: false }
  if (path === '/api/config/ui') {
    data = { dashboard_title: 'Probe', dashboard_layout_mode: 'standard' }
  }
  return route.fulfill({
    contentType: 'application/json',
    body: JSON.stringify(data),
  })
})
await page.addInitScript(() => {
  const metrics = { longTasks: [], loafs: [] }
  window.__lt = metrics
  const scriptsOf = (entry) =>
    (entry.scripts ?? [])
      .map((s) => ({
        url: (s.sourceURL || s.sourceFunctionName || '').split('/').pop() || '',
        ms: Math.round(s.duration ?? 0),
        invoker: s.invoker,
      }))
      .filter((s) => s.url)
      .sort((a, b) => b.ms - a.ms)
      .slice(0, 3)
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        metrics.longTasks.push({
          t: Math.round(e.startTime),
          ms: Math.round(e.duration),
        })
      }
    }).observe({ type: 'longtask', buffered: true })
  } catch {}
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        metrics.loafs.push({
          t: Math.round(e.startTime),
          ms: Math.round(e.duration),
          blocking: Math.round(e.blockingDuration ?? 0),
          scripts: scriptsOf(e),
        })
      }
    }).observe({ type: 'long-animation-frame', buffered: true })
  } catch {}
})

await page.goto(`${origin}/`, { waitUntil: 'domcontentloaded', timeout: 30_000 })

const started = Date.now()
let prevLt = 0
let prevLoaf = 0
while (Date.now() - started <= TOTAL_MS) {
  const m = await page.evaluate(() => structuredClone(window.__lt))
  const now = Math.round(await page.evaluate(() => performance.now()))
  const hot = {}
  for (const l of m.loafs) {
    for (const s of l.scripts) hot[s.url] = (hot[s.url] || 0) + 1
  }
  const row = {
    wallMs: Date.now() - started,
    now,
    longTaskCount: m.longTasks.length,
    longTaskDelta: m.longTasks.length - prevLt,
    longTaskMax: Math.max(0, ...m.longTasks.map((t) => t.ms)),
    lastTask: m.longTasks.at(-1) ?? null,
    loafCount: m.loafs.length,
    loafDelta: m.loafs.length - prevLoaf,
    loafMax: Math.max(0, ...m.loafs.map((t) => t.ms)),
    lastLoaf: m.loafs.at(-1) ?? null,
    hotScripts: Object.entries(hot)
      .toSorted((a, b) => b[1] - a[1])
      .slice(0, 8),
  }
  prevLt = m.longTasks.length
  prevLoaf = m.loafs.length
  appendFileSync(out, `${JSON.stringify(row)}\n`)
  const remain = TOTAL_MS - (Date.now() - started)
  if (remain <= 0) break
  await page.waitForTimeout(Math.min(SAMPLE_MS, remain))
}

await browser.close()
server.kill()
appendFileSync(out, `${JSON.stringify({ done: true, wallMs: Date.now() - started })}\n`)
