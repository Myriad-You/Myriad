// Offscreen renderer for the README animations: the production frontend, a local
// instance behind it, a virtual clock, one screenshot per frame.
import { Buffer } from 'node:buffer'
import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { chromium } from '@playwright/test'
import sharp from 'sharp'

/** Where the production build is served (`node scripts/spa-server.mjs`). */
export const APP = process.env.README_MEDIA_APP || 'http://localhost:4391'
/** The local instance the frontend talks to. */
export const BACKEND = process.env.README_MEDIA_BACKEND || 'http://127.0.0.1:1199'
/** Frames and encodes land here, outside the repository. */
export const WORK = process.env.README_MEDIA_WORK || path.join(os.tmpdir(), 'myriad-readme-media')
/** The virtual clock starts here, in the browser's time zone (Asia/Tokyo). */
export const START = new Date('2026-10-23T10:20:00+09:00')

/** Stand-ins, checked first: `{ url: '/api/x' | RegExp, method?, body | (req, url) => body, status?, ct? }`. */
export const mocks = []
/** Rewrites of the instance's JSON answers: `{ test: RegExp, fn: (json, req, url) => json }`. */
export const overrides = []

// Paths the instance answers; everything else on APP is the built frontend.
const BACKEND_PATH = /^\/(api|health|ready|users|\.well-known|nodeinfo|inbox|federation|media\/|blog(\/|$))/

async function route(r) {
  const req = r.request()
  const u = new URL(req.url())
  if (u.protocol === 'data:' || u.protocol === 'blob:') return r.continue()
  const own = u.origin === APP
  if (own && !BACKEND_PATH.test(u.pathname)) return r.continue()
  for (const m of mocks) {
    if (m.method && m.method !== req.method()) continue
    const target = own ? u.pathname + u.search : req.url()
    const hit = typeof m.url === 'string' ? target === m.url || u.pathname === m.url : m.url.test(target)
    if (!hit) continue
    const v = typeof m.body === 'function' ? await m.body(req, u) : m.body
    const body = Buffer.isBuffer(v) || typeof v === 'string' ? v : JSON.stringify(v)
    return r.fulfill({ status: m.status || 200, contentType: m.ct || 'application/json', headers: { 'access-control-allow-origin': '*' }, body })
  }
  // Nothing leaves the machine: outside requests without a stand-in fail.
  if (!own) return r.abort()
  try {
    const resp = await r.fetch({ url: BACKEND + u.pathname + u.search, timeout: 60000, maxRedirects: 0 })
    const ov = overrides.filter((o) => o.test.test(u.pathname + u.search))
    if (ov.length && resp.ok()) {
      let json = JSON.parse((await resp.body()).toString('utf8'))
      for (const o of ov) json = await o.fn(json, req, u)
      return r.fulfill({ status: resp.status(), contentType: 'application/json', body: JSON.stringify(json) })
    }
    return r.fulfill({ response: resp })
  } catch (error) {
    console.warn('backend request failed', u.pathname, error.message.slice(0, 80))
    return r.abort()
  }
}

// Every first-run tour, marked as seen.
const TOURS = ['config-ai-persona-owner', 'config-owner', 'config-persona-owner', 'home-edit-owner', 'home-owner', 'home-visitor', 'library-visitor', 'library-owner', 'library-canvas-visitor', 'library-canvas-owner', 'tapp-detail-visitor', 'tapp-detail-owner', 'tapp-owner', 'tapp-visitor', 'tapp-playground-owner', 'tapp-store-visitor', 'tapp-store-owner', 'blog-visitor', 'blog-owner']

export async function open({ width = 1440, height = 900, dpr = 1.5, scheme = 'light', locale = 'zh-CN', state, chromeArgs = [], noClock = false } = {}) {
  const browser = await chromium.launch({ channel: 'chrome', args: ['--ignore-gpu-blocklist', '--autoplay-policy=no-user-gesture-required', ...chromeArgs] })
  const ctx = await browser.newContext({ storageState: state, locale, viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: scheme, timezoneId: 'Asia/Tokyo', serviceWorkers: 'block' })
  await ctx.route('**/*', route)
  // Sockets stay closed unless a scene lets them through to the instance (WS_PASS).
  await ctx.routeWebSocket(/.*/, (ws) => (process.env.WS_PASS && new RegExp(process.env.WS_PASS).test(ws.url()) ? ws.connectToServer() : ws.close()))
  await ctx.addInitScript(() => {
    let a = 0x9E3779B9
    Math.random = () => { a |= 0; a = (a + 0x6D2B79F5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296 }
  })
  await ctx.addInitScript((tours) => { try { localStorage.setItem('myriad_tour_done_v1', JSON.stringify(tours)) } catch {} }, TOURS)
  const page = await ctx.newPage()
  if (!noClock) {
    // install() alone lets time run; pauseAt() stops it so only step() moves it.
    await page.clock.install({ time: START })
    await page.clock.pauseAt(new Date(START.getTime() + 1000))
  }
  const cdp = await ctx.newCDPSession(page)
  await cdp.send('Animation.enable')
  page.__cdp = cdp
  page.__dpr = dpr
  return { browser, ctx, page }
}

// Advance virtual time by dt ms: timers and rAF through the fake clock, CSS and WAAPI through currentTime.
export async function step(page, dt) {
  // A Tapp preview frame refuses the clock's script; the other frames have still advanced.
  try { await page.clock.runFor(dt) } catch (e) { if (!/eval is disabled/.test(e.message)) throw e }
  await page.evaluate((dt) => {
    // Animations run on the document timeline, which the fake clock does not hold. Keep each
    // at a near-zero rate so real capture time does not move it, then place it at its virtual
    // time. The rate stays above zero, so reaching the end still finishes it.
    const seen = (window.__seen ||= new WeakMap())
    const EPS = 1e-6
    for (const a of document.getAnimations()) {
      let st = seen.get(a)
      if (!st) {
        st = { t: Number(a.currentTime) || 0, rate: a.playbackRate || 1 }
        seen.set(a, st)
        try { a.playbackRate = EPS * Math.sign(st.rate || 1) } catch {}
      } else if (Math.abs(a.playbackRate) > EPS * 2) {
        st.rate = a.playbackRate
        try { a.playbackRate = EPS * Math.sign(st.rate) } catch {}
      }
      if (a.playState === 'finished' || a.playState === 'idle' || a.playState === 'paused') continue
      st.t += dt * st.rate
      try { a.currentTime = st.t } catch {}
    }
  }, dt)
}

export async function settle(page, ms, dt = 50) {
  for (let t = 0; t < ms; t += dt) { await step(page, dt); await page.waitForTimeout(5) }
}

// Real time (the virtual clock stays put) until every visible image is decoded, so no frame shows a hole.
export async function imagesDecoded(page, maxMs = 4000) {
  const t0 = Date.now()
  while (Date.now() - t0 < maxMs) {
    const pending = await page.evaluate(async () => {
      const vw = innerWidth; const vh = innerHeight
      const shown = [...document.images].filter((i) => { const r = i.getBoundingClientRect(); return r.width > 4 && r.height > 4 && r.bottom > 0 && r.top < vh && r.right > 0 && r.left < vw })
      // A lazy image only counts once it is actually on top somewhere.
      const onTop = (i) => { const r = i.getBoundingClientRect(); const x = Math.min(vw - 1, Math.max(0, r.x + r.width / 2)); const y = Math.min(vh - 1, Math.max(0, r.y + r.height / 2)); return document.elementsFromPoint(x, y).includes(i) }
      const bad = shown.filter((i) => !(i.complete && i.naturalWidth > 0) && i.src && !i.src.startsWith('data:') && (i.loading !== 'lazy' || onTop(i)))
      await Promise.all(shown.filter((i) => i.complete && i.naturalWidth > 0).map((i) => i.decode().catch(() => {})))
      return bad.length
    })
    if (!pending) return
    await page.waitForTimeout(60)
  }
}

export async function shoot(page, clip) {
  const vp = page.viewportSize()
  const off = await page.evaluate(() => ({ x: scrollX, y: scrollY }))
  const c = clip || { x: 0, y: 0, width: vp.width, height: vp.height }
  // Page.captureScreenshot takes document coordinates and CSS pixels unless scale is given.
  const r = await page.__cdp.send('Page.captureScreenshot', { format: 'jpeg', quality: 94, clip: { ...c, x: c.x + off.x, y: c.y + off.y, scale: page.__dpr } })
  return Buffer.from(r.data, 'base64')
}

/** Record `ms` of virtual time at `fps`, calling `script(t)` before each frame. */
export async function capture(page, { ms, fps = 10, script, clip } = {}) {
  const dt = 1000 / fps
  const frames = []
  for (let t = 0; t < ms; t += dt) {
    if (script) await script(t)
    await step(page, dt)
    await imagesDecoded(page)
    frames.push(await shoot(page, clip))
  }
  return { frames, dt }
}

const framesDir = (name) => path.join(WORK, 'frames', name)
export function saveFrames(name, rec) {
  const dir = framesDir(name)
  fs.rmSync(dir, { recursive: true, force: true })
  fs.mkdirSync(dir, { recursive: true })
  rec.frames.forEach((f, i) => fs.writeFileSync(path.join(dir, `${String(i).padStart(4, '0')}.jpg`), f))
  fs.writeFileSync(path.join(dir, 'meta.json'), JSON.stringify({ dt: rec.dt }))
}
export function loadFrames(name) {
  const dir = framesDir(name)
  const files = fs.readdirSync(dir).filter((f) => f.endsWith('.jpg')).sort()
  return { frames: files.map((f) => fs.readFileSync(path.join(dir, f))), dt: JSON.parse(fs.readFileSync(path.join(dir, 'meta.json'), 'utf8')).dt }
}
export function recorded() {
  return fs.existsSync(path.join(WORK, 'frames')) ? fs.readdirSync(path.join(WORK, 'frames')).sort() : []
}

/** Animated WebP. Every n-th frame is kept; identical neighbours merge into one longer frame. */
export async function encode(out, { frames, dt }, { width, quality, crop, every = 1 }) {
  // `every` may depend on the frame index, so quiet stretches can drop frames while the rest keeps full rate.
  const step = typeof every === 'function' ? every : () => every
  const runs = []
  for (let i = 0; i < frames.length; i += step(i)) {
    const h = crypto.createHash('sha1').update(frames[i]).digest('hex')
    if (runs.length && runs.at(-1).h === h) runs.at(-1).n += step(i)
    else runs.push({ h, f: frames[i], n: step(i) })
  }
  const bufs = []
  for (const r of runs) {
    let img = sharp(r.f)
    if (crop) img = sharp(await img.extract(crop).png().toBuffer())
    bufs.push(await img.resize({ width, kernel: 'lanczos3' }).png().toBuffer())
  }
  const delay = runs.map((r) => Math.round(r.n * dt))
  await sharp(bufs, { join: { animated: true } }).webp({ loop: 0, delay, quality, effort: 4, smartSubsample: true }).toFile(out)
  return fs.statSync(out).size
}

// A visible pointer: the real one never shows up in screenshots.
export async function addCursor(ctx) {
  await ctx.addInitScript(() => {
    addEventListener('DOMContentLoaded', () => {
      const c = document.createElement('div')
      c.style.cssText = `position:fixed;left:0;top:0;width:22px;height:22px;margin:-3px 0 0 -3px;z-index:2147483647;pointer-events:none;opacity:0;`
        + `background:url("data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 22 22"><path d="M3 2l15 9-7 1.6L7.6 19z" fill="#111" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg>')}") no-repeat;filter:drop-shadow(0 1px 2px rgba(0,0,0,.35))`
      document.documentElement.appendChild(c)
      const ring = document.createElement('div')
      ring.style.cssText = 'position:fixed;width:34px;height:34px;margin:-17px 0 0 -17px;border-radius:50%;border:2px solid rgba(255,255,255,.95);box-shadow:0 0 0 2px rgba(0,0,0,.15);z-index:2147483646;pointer-events:none;opacity:0;transform:scale(.4)'
      document.documentElement.appendChild(ring)
      addEventListener('pointermove', (e) => { c.style.opacity = '1'; c.style.transform = `translate(${e.clientX}px,${e.clientY}px)` }, true)
      addEventListener('pointerdown', (e) => { ring.style.left = `${e.clientX}px`; ring.style.top = `${e.clientY}px`; ring.animate([{ opacity: 1, transform: 'scale(.4)' }, { opacity: 0, transform: 'scale(1.3)' }], { duration: 450, easing: 'ease-out' }) }, true)
    })
  })
}

/**
 * Actions on the virtual timeline: `{ at, run }` runs once when t passes `at`;
 * `{ at, ms, glide: [x, y] | () => [x, y], from? }` moves the pointer there with easing.
 */
export function timeline(page, items) {
  const done = new Set()
  let pos = { x: 720, y: 450 }
  return async (t) => {
    for (const [i, it] of items.entries()) {
      if (it.glide) {
        if (t < it.at || t > it.at + it.ms + 1) continue
        const k = Math.min(1, (t - it.at) / it.ms)
        const e = k < 0.5 ? 2 * k * k : 1 - (-2 * k + 2) ** 2 / 2
        it._from ||= it.from || [pos.x, pos.y]
        const g = typeof it.glide === 'function' ? it.glide() : it.glide
        const x = it._from[0] + (g[0] - it._from[0]) * e; const y = it._from[1] + (g[1] - it._from[1]) * e
        await page.mouse.move(x, y)
        if (k === 1) pos = { x, y }
        continue
      }
      if (!done.has(i) && t >= it.at) { done.add(i); await it.run() }
    }
  }
}

/** One timeline item per character, so typing shows frame by frame; '\n' presses Enter. */
export function typing(page, at, text, dt = 70) {
  return Array.from(text).map((ch, i) => ({ at: at + i * dt, run: () => (ch === '\n' ? page.keyboard.press('Enter') : page.keyboard.insertText(ch)) }))
}
