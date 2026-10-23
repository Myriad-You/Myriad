// Synthetic, neutral artwork for demo data: gradients, shapes and a title. Deterministic per seed.
import { Buffer } from 'node:buffer'
import crypto from 'node:crypto'
import sharp from 'sharp'

const PALETTES = [
  ['#f6d5e5', '#b9a7f2', '#5b4b9a'], ['#cfe8ff', '#7fb2f0', '#2c4a7d'], ['#ffe3c4', '#f4a582', '#8c3b2e'],
  ['#d9f2e3', '#7cc6a4', '#2f6b55'], ['#fff1b8', '#f2c14e', '#7a5a12'], ['#e9e4ff', '#9a8cf0', '#3a2f7a'],
  ['#ffd6d6', '#f28b9b', '#7d2a3c'], ['#d6f5f5', '#6cc3c9', '#24575c'], ['#efe7da', '#c9a27e', '#5c4330'],
  ['#1f2236', '#4b4f8a', '#c7c9ff'], ['#21302a', '#4f7a63', '#d4f0df'], ['#2b1d2f', '#8a4f7d', '#ffd1ef'],
]
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]))
function rng(seed) { const h = crypto.createHash('sha1').update(String(seed)).digest(); let i = 0; return () => h[(i++) % h.length] / 255 }
export async function art({ seed, title = '', sub = '', w = 400, h = 600, style }) {
  const r = rng(seed); const [a, b, c] = PALETTES[Math.floor(r() * PALETTES.length)]
  const st = style ?? Math.floor(r() * 4)
  const shapes = []
  if (st === 0) { for (let k = 0; k < 6; k++) shapes.push(`<circle cx="${r() * w}" cy="${r() * h}" r="${(0.1 + r() * 0.35) * Math.max(w, h)}" fill="${[a, b, c][k % 3]}" opacity="${0.25 + r() * 0.35}"/>`)
}
  if (st === 1) { for (let k = 0; k < 7; k++) { const y = h * (0.2 + k * 0.12); shapes.push(`<path d="M0 ${y} C ${w * 0.3} ${y - h * 0.12 * r()}, ${w * 0.6} ${y + h * 0.12 * r()}, ${w} ${y - h * 0.05}" stroke="${[b, c][k % 2]}" stroke-width="${2 + r() * 10}" fill="none" opacity="0.55"/>`) }
}
  if (st === 2) { for (let k = 0; k < 18; k++) { const s = (0.05 + r() * 0.12) * w; shapes.push(`<rect x="${r() * w}" y="${r() * h}" width="${s}" height="${s}" rx="${s * 0.2}" fill="${[a, b, c][k % 3]}" opacity="${0.3 + r() * 0.5}" transform="rotate(${r() * 40 - 20} ${w / 2} ${h / 2})"/>`) }
}
  if (st === 3) { shapes.push(`<circle cx="${w * (0.3 + r() * 0.4)}" cy="${h * 0.38}" r="${Math.min(w, h) * 0.26}" fill="${a}" opacity="0.9"/>`); for (let k = 0; k < 4; k++) shapes.push(`<path d="M0 ${h * (0.6 + k * 0.08)} L ${w * (0.25 + r() * 0.2)} ${h * (0.5 + k * 0.06)} L ${w * (0.6 + r() * 0.2)} ${h * (0.62 + k * 0.05)} L ${w} ${h * (0.55 + k * 0.07)} L ${w} ${h} L 0 ${h} Z" fill="${[b, c][k % 2]}" opacity="${0.5 + k * 0.1}"/>`) }
  const fs = Math.round(Math.min(w, h) * (title.length > 8 ? 0.09 : 0.12))
  const dark = /^#[0-3]/.test(a)
  const ink = dark ? '#ffffff' : '#1b1a2e'
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient>
<linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0.55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="${dark ? 0.35 : 0.18}"/></linearGradient></defs>
<rect width="100%" height="100%" fill="url(#g)"/>${shapes.join('')}<rect width="100%" height="100%" fill="url(#s)"/>
${title ? `<text x="${w * 0.07}" y="${h - h * 0.1 - (sub ? fs * 0.9 : 0)}" font-family="PingFang SC, Hiragino Sans, Helvetica Neue, sans-serif" font-weight="700" font-size="${fs}" fill="${ink}">${esc(title)}</text>` : ''}
${sub ? `<text x="${w * 0.07}" y="${h - h * 0.1}" font-family="PingFang SC, Hiragino Sans, Helvetica Neue, sans-serif" font-size="${Math.round(fs * 0.5)}" fill="${ink}" opacity="0.8">${esc(sub)}</text>` : ''}
</svg>`
  return sharp(Buffer.from(svg)).png().toBuffer()
}
export async function avatar({ seed, label = '', size = 160 }) {
  const r = rng(seed); const [a, , c] = PALETTES[Math.floor(r() * 9)]
  const cells = []; for (let y = 0; y < 5; y++) { for (let x = 0; x < 3; x++) { if (r() > 0.5) { const s = size / 5; cells.push(`<rect x="${x * s}" y="${y * s}" width="${s}" height="${s}" fill="${c}" opacity="0.85"/>`, `<rect x="${(4 - x) * s}" y="${y * s}" width="${s}" height="${s}" fill="${c}" opacity="0.85"/>`) }
}
}
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}"><rect width="100%" height="100%" fill="${a}"/><g transform="translate(${size * 0.15} ${size * 0.15}) scale(0.7)">${cells.join('')}</g>${label ? `<text x="50%" y="58%" text-anchor="middle" font-family="PingFang SC, Hiragino Sans, sans-serif" font-weight="700" font-size="${size * 0.42}" fill="#fff" opacity="0">${esc(label)}</text>` : ''}</svg>`
  return sharp(Buffer.from(svg)).png().toBuffer()
}
