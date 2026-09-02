/**
 * 思考态流光的现场抽签。
 *
 * 不用平移色带。多团独立色块，但同时在场的数量和底边范围是锁死的：
 * 五团铺满左右，高度贴空闲色带（约 88px），不会顶满整段光罩。
 */

export type AuroraRand = () => number

export const AURORA_BLOB_COUNT = 5
export const AURORA_HUE_COUNT = { min: 4, max: 5 } as const
/** 对齐空闲色带：`::after` 高 88px、bottom -28px。 */
export const AURORA_BLOB_HEIGHT = { min: 76, max: 92 } as const
export const AURORA_BLOB_BOTTOM = { min: -28, max: -18 } as const

export const BLOB_PATHS = [
  'agent-panel-aurora-blob-a',
  'agent-panel-aurora-blob-b',
  'agent-panel-aurora-blob-c',
  'agent-panel-aurora-blob-d',
] as const

export interface AuroraBlobPaint {
  color: string
  x: string
  y: string
  w: string
  h: string
  blur: string
  opacity: string
  path: (typeof BLOB_PATHS)[number]
  ms: string
  delay: string
  dir: 'normal' | 'reverse'
  stagger: string
}

export interface AuroraPrismPaint {
  blobs: AuroraBlobPaint[]
}

function hueDelta(a: number, b: number): number {
  const d = Math.abs(a - b) % 360
  return Math.min(d, 360 - d)
}

function pickHues(count: number, rand: AuroraRand): number[] {
  const hues: number[] = []
  let guard = 0
  while (hues.length < count && guard < 96) {
    guard += 1
    const hue = rand() * 360
    if (hues.some((seen) => hueDelta(seen, hue) < 32)) continue
    hues.push(hue)
  }
  while (hues.length < count) hues.push(rand() * 360)
  return hues
}

function mixColor(hue: number, rand: AuroraRand, dark: boolean): string {
  const light = dark ? 58 + rand() * 16 : 64 + rand() * 16
  const chroma = dark ? 0.22 + rand() * 0.1 : 0.19 + rand() * 0.1
  const amount = 80 + Math.round(rand() * 14)
  const wall =
    rand() < 0.34
      ? 'var(--color-accent)'
      : rand() < 0.5
        ? 'var(--color-secondary)'
        : 'var(--color-primary)'
  return `color-mix(in oklab, oklch(${light.toFixed(1)}% ${chroma.toFixed(3)} ${hue.toFixed(1)}deg) ${amount}%, ${wall})`
}

function shuffle<T>(items: readonly T[], rand: AuroraRand): T[] {
  const next = [...items]
  for (let i = next.length - 1; i > 0; i -= 1) {
    const j = Math.floor(rand() * (i + 1))
    const left = next[i]
    const right = next[j]
    if (left === undefined || right === undefined) continue
    next[i] = right
    next[j] = left
  }
  return next
}

function paintBlobs(colors: string[], rand: AuroraRand): AuroraBlobPaint[] {
  const paths = shuffle(BLOB_PATHS, rand)
  const dirs = shuffle(
    ['normal', 'reverse', 'normal', 'reverse', 'normal'] as const,
    rand,
  )
  return Array.from({ length: AURORA_BLOB_COUNT }, (_, index) => {
    const color =
      colors[index % colors.length] ??
      colors[0] ??
      'var(--color-primary)'
    const width = 34 + rand() * 8
    const slot = ((index + 0.5) / AURORA_BLOB_COUNT) * 100
    const center = slot + (rand() - 0.5) * 10
    const x = center - width / 2
    return {
      color,
      x: `${x.toFixed(1)}%`,
      y: `${(-28 + rand() * 10).toFixed(1)}px`,
      w: `${width.toFixed(1)}%`,
      h: `${(76 + rand() * 16).toFixed(1)}px`,
      blur: `${(18 + rand() * 8).toFixed(1)}px`,
      opacity: (0.62 + rand() * 0.28).toFixed(2),
      path: paths[index % paths.length] ?? 'agent-panel-aurora-blob-a',
      ms: `${(5.2 + rand() * 3.6).toFixed(2)}s`,
      delay: `${(-rand() * 6).toFixed(2)}s`,
      dir: dirs[index] ?? 'normal',
      stagger: String(index),
    }
  })
}

export function paintAuroraPrism(
  rand: AuroraRand = Math.random,
  dark = false,
): AuroraPrismPaint {
  const hueCount =
    AURORA_HUE_COUNT.min +
    Math.floor(rand() * (AURORA_HUE_COUNT.max - AURORA_HUE_COUNT.min + 1))
  const colors = pickHues(hueCount, rand).map((hue) =>
    mixColor(hue, rand, dark),
  )
  return { blobs: paintBlobs(colors, rand) }
}

export function applyAuroraPrism(
  el: HTMLElement,
  paint: AuroraPrismPaint,
): void {
  el.replaceChildren()
  for (const blob of paint.blobs) {
    const node = document.createElement('span')
    node.className = 'agent-panel-aurora-blob'
    node.style.setProperty('--blob-color', blob.color)
    node.style.setProperty('--blob-x', blob.x)
    node.style.setProperty('--blob-y', blob.y)
    node.style.setProperty('--blob-w', blob.w)
    node.style.setProperty('--blob-h', blob.h)
    node.style.setProperty('--blob-blur', blob.blur)
    node.style.setProperty('--blob-opacity', blob.opacity)
    node.style.setProperty('--blob-path', blob.path)
    node.style.setProperty('--blob-ms', blob.ms)
    node.style.setProperty('--blob-delay', blob.delay)
    node.style.setProperty('--blob-dir', blob.dir)
    node.style.setProperty('--blob-stagger', blob.stagger)
    el.append(node)
  }
}
