import type {
  LibraryCanvasTransform,
  LibraryCanvasViewport,
} from '../../utils/libraryCanvas'
import { getLibraryCanvasFocusScaleAt } from '../../utils/libraryCanvas'

export interface CanvasCardPaintNode {
  el: HTMLElement
  cx: number
  cy: number
  lastFocus: number
  lastZ: number
}

export interface CanvasCardPaintCache {
  world: HTMLElement | null
  childCount: number
  first: Element | null
  last: Element | null
  nodes: CanvasCardPaintNode[]
}

/** Skip a style write when the focus scale would not move a pixel of stacking. */
export const CANVAS_FOCUS_WRITE_EPS = 0.004

export function createCanvasCardPaintCache(): CanvasCardPaintCache {
  return {
    world: null,
    childCount: -1,
    first: null,
    last: null,
    nodes: [],
  }
}

export function resetCanvasCardPaintCache(cache: CanvasCardPaintCache): void {
  cache.world = null
  cache.childCount = -1
  cache.first = null
  cache.last = null
  cache.nodes = []
}

export function canvasCardPaintCacheIsCurrent(
  cache: CanvasCardPaintCache,
  world: HTMLElement,
): boolean {
  return (
    cache.world === world &&
    cache.childCount === world.childElementCount &&
    cache.first === world.firstElementChild &&
    cache.last === world.lastElementChild &&
    cache.nodes.length === world.childElementCount
  )
}

export function refreshCanvasCardPaintCache(
  cache: CanvasCardPaintCache,
  world: HTMLElement,
): void {
  const prevByEl = new Map<HTMLElement, CanvasCardPaintNode>()
  for (let i = 0; i < cache.nodes.length; i++) {
    const prev = cache.nodes[i]!
    prevByEl.set(prev.el, prev)
  }
  const nodes: CanvasCardPaintNode[] = []
  const children = world.children
  for (let i = 0; i < children.length; i++) {
    const el = children[i] as HTMLElement
    if (!el.hasAttribute('data-canvas-card')) continue
    const left = Number(el.dataset.layoutLeft)
    const top = Number(el.dataset.layoutTop)
    const width = Number(el.dataset.layoutWidth)
    const height = Number(el.dataset.layoutHeight)
    if (
      !Number.isFinite(left) ||
      !Number.isFinite(top) ||
      !Number.isFinite(width) ||
      !Number.isFinite(height)
    ) {
      continue
    }
    const prev = prevByEl.get(el)
    nodes.push({
      el,
      cx: left + width / 2,
      cy: top + height / 2,
      lastFocus: prev?.lastFocus ?? Number.NaN,
      lastZ: prev?.lastZ ?? -1,
    })
  }
  cache.world = world
  cache.childCount = world.childElementCount
  cache.first = world.firstElementChild
  cache.last = world.lastElementChild
  cache.nodes = nodes
}

export function paintCanvasCardFocus(
  nodes: readonly CanvasCardPaintNode[],
  transform: LibraryCanvasTransform,
  viewport: LibraryCanvasViewport,
): void {
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i]!
    const focus = getLibraryCanvasFocusScaleAt(
      node.cx,
      node.cy,
      transform,
      viewport,
    )
    const z = Math.round(focus * 100)
    if (
      Number.isFinite(node.lastFocus) &&
      Math.abs(focus - node.lastFocus) < CANVAS_FOCUS_WRITE_EPS &&
      z === node.lastZ
    ) {
      continue
    }
    node.lastFocus = focus
    node.lastZ = z
    node.el.style.transform = `scale(${focus})`
    node.el.style.zIndex = String(z)
  }
}

export function sameLibraryItemIds(
  prev: readonly { id: string }[] | null,
  next: readonly { id: string }[],
): boolean {
  if (!prev || prev.length !== next.length) return false
  for (let i = 0; i < next.length; i++) {
    if (prev[i]!.id !== next[i]!.id) return false
  }
  return true
}

export function canvasFollowTargetsNeedPaint(
  lastPose: { x: number; y: number; scale: number } | null,
  next: { x: number; y: number; scale: number },
  lastWorld: HTMLElement | null,
  world: HTMLElement | null,
): boolean {
  if (lastWorld !== world) return true
  if (!lastPose) return true
  return (
    lastPose.x !== next.x ||
    lastPose.y !== next.y ||
    lastPose.scale !== next.scale
  )
}
