/**
 * Rebuild `public/` WebP from `raw/` originals (idempotent).
 * `public/icons/oauth/*`, `public/favicon.webp`, and `*.svg` wordmarks are not processed.
 *
 * cap 192 = 64px display × 3 (DPR3); one WebP, no srcset.
 * saturation 1.05 for painted UI icons; brand assets stay 1.0.
 * HSR wordmark is a CSS mask-image — lossless WebP so alpha edges stay crisp.
 */
import { existsSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'

const FE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const RAW = path.join(FE, 'raw')
const PUB = path.join(FE, 'public')

const PAINTED_DIRS = [
  'config', 'notifications', 'control-panel', 'dynamic', 'greeting',
  'status', 'weather', 'tapp', 'widgets', 'brew',
]

const kb = (b) => `${(b / 1024).toFixed(1)}KB`

async function encode(src, out, { cap, saturation, quality, lossless }) {
  let pipe = sharp(src).resize({ width: cap, height: cap, fit: 'inside', withoutEnlargement: true })
  if (saturation !== 1) pipe = pipe.modulate({ saturation })
  pipe = lossless ? pipe.webp({ lossless: true, effort: 6 }) : pipe.webp({ quality, effort: 6 })
  await pipe.toFile(out)
  return statSync(out).size
}

let count = 0
let rawBytes = 0
let webpBytes = 0

async function run(label, src, out, opts) {
  const o = statSync(src).size
  const n = await encode(src, out, opts)
  rawBytes += o
  webpBytes += n
  count++
  console.log(`  ${label.padEnd(40)} ${kb(o).padStart(9)} → ${kb(n).padStart(8)}  ${((1 - n / o) * 100).toFixed(0)}%`)
}

for (const dir of PAINTED_DIRS) {
  const rawDir = path.join(RAW, 'icons', dir)
  if (!existsSync(rawDir)) {
    console.warn(`  ! missing raw/icons/${dir} — skipped`)
    continue
  }
  for (const f of readdirSync(rawDir).filter((x) => x.endsWith('.png'))) {
    await run(`icons/${dir}/${f}`,
      path.join(rawDir, f),
      path.join(PUB, 'icons', dir, f.replaceAll(/\.png$/g, '.webp')),
      { cap: 192, saturation: 1.05, quality: 90 })
  }
}

// starrail.png is a CSS mask-image → lossless WebP at a larger cap so the alpha silhouette stays crisp.
const gameRaw = path.join(RAW, 'game-logos')
if (existsSync(gameRaw)) {
  for (const f of readdirSync(gameRaw).filter((x) => x.endsWith('.png'))) {
    const isMask = f === 'starrail.png'
    await run(`game-logos/${f}${isMask ? ' [mask]' : ''}`,
      path.join(gameRaw, f),
      path.join(PUB, 'game-logos', f.replaceAll(/\.png$/g, '.webp')),
      isMask ? { cap: 400, saturation: 1.0, lossless: true } : { cap: 192, saturation: 1.0, quality: 90 })
  }
}

// App logo: no saturation bump; cap for ≤120px display.
const logoRaw = path.join(RAW, 'logo.webp')
if (existsSync(logoRaw)) {
  await run('logo.webp', logoRaw, path.join(PUB, 'logo.webp'), { cap: 360, saturation: 1.0, quality: 90 })
}

console.log(`\n${count} images: ${kb(rawBytes)} → ${kb(webpBytes)}  (${((1 - webpBytes / rawBytes) * 100).toFixed(1)}% smaller)`)
