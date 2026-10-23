// Encodes every recorded variant with its scene's settings: `node encode.mjs <outDir> [scene ...]`.
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import sharp from 'sharp'
import { CHAT } from './content.mjs'
import { encode, loadFrames, recorded } from './render.mjs'
import { chatReplyAt } from './stream.mjs'

// crop is [left, top, width, height] in recorded pixels (1440×900 at 1.5×).
const SCENES = {
  home: { width: 1100, quality: 55, still: 60 },
  // Half rate until her answer starts; from there the mouth keeps every frame (10 fps).
  chat: { width: 600, quality: 42, crop: [640, 470, 960, 880], every: (L) => (i) => (i < chatReplyAt(CHAT[L].ask) / 100 - 2 ? 2 : 1) },
  face: { width: 600, quality: 45, every: 2, crop: [150, 230, 1500, 980] },
  report: { width: 800, quality: 50 },
  library: { width: 720, quality: 40 },
  tapp: { width: 800, quality: 50 },
  // The blog theme has its own light/dark switch and ignores the system scheme, so only light is used.
  blog: { width: 800, quality: 50, schemes: ['light'] },
}
const [outDir = 'out', ...only] = process.argv.slice(2)
fs.mkdirSync(outDir, { recursive: true })
for (const name of recorded()) {
  const m = name.match(/^(\w+)-(zh|ja|en)-(light|dark)$/)
  const o = m && SCENES[m[1]]
  if (!o || (only.length && !only.includes(m[1])) || (o.schemes && !o.schemes.includes(m[3]))) continue
  const crop = o.crop && { left: o.crop[0], top: o.crop[1], width: o.crop[2], height: o.crop[3] }
  const rec = loadFrames(name)
  const size = await encode(path.join(outDir, `${name}.webp`), rec, { width: o.width, quality: o.quality, every: typeof o.every === 'function' ? o.every(m[2]) : o.every || 1, crop })
  console.log(name, `${Math.round(size / 1024)} KB`)
  if (o.still != null) {
    // The reduced-motion still for the hero.
    let img = sharp(rec.frames[Math.min(o.still, rec.frames.length - 1)])
    if (crop) img = sharp(await img.extract(crop).png().toBuffer())
    await img.resize({ width: o.width }).webp({ quality: 70 }).toFile(path.join(outDir, `${name}-still.webp`))
  }
}
