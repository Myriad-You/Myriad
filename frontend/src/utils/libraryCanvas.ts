export interface LibraryCanvasLayout {
  left: number
  top: number
  width: number
  height: number
  gridX: number
  gridY: number
  gridW: number
  gridH: number
}

export interface LibraryCanvasTransform {
  x: number
  y: number
  scale: number
}

export interface LibraryCanvasViewport {
  width: number
  height: number
}

export interface LibraryCanvasBounds {
  minX: number
  maxX: number
  minY: number
  maxY: number
}

export const LIBRARY_CANVAS_CELL_SIZE = 184
export const LIBRARY_CANVAS_GAP = 16
export const LIBRARY_CANVAS_STRIDE =
  LIBRARY_CANVAS_CELL_SIZE + LIBRARY_CANVAS_GAP
export const LIBRARY_CANVAS_FOCUS_MIN_SCALE = 0.62
export const LIBRARY_CANVAS_FOCUS_MAX_SCALE = 1.3
const LIBRARY_CANVAS_FOCUS_EXTENT = 1.7

function appendSquareRing(candidates: Array<[number, number]>, radius: number) {
  for (let x = -radius; x <= radius; x++) candidates.push([x, -radius])
  for (let y = -radius + 1; y <= radius; y++) candidates.push([radius, y])
  for (let x = radius - 1; x >= -radius; x--) candidates.push([x, radius])
  for (let y = radius - 1; y > -radius; y--) candidates.push([-radius, y])
}

export function buildCenterOutCanvasLayout<T extends { id: string }>(
  items: T[],
  getGridSize: (item: T) => { w: number; h: number },
) {
  const occupied = new Set<string>()
  const layouts = new Map<string, LibraryCanvasLayout>()
  const candidates: Array<[number, number]> = [[0, 0]]
  let candidateRadius = 0

  const fits = (x: number, y: number, w: number, h: number) => {
    for (let dx = 0; dx < w; dx++) {
      for (let dy = 0; dy < h; dy++) {
        if (occupied.has(`${x + dx},${y + dy}`)) return false
      }
    }
    return true
  }

  const occupy = (x: number, y: number, w: number, h: number) => {
    for (let dx = 0; dx < w; dx++) {
      for (let dy = 0; dy < h; dy++) occupied.add(`${x + dx},${y + dy}`)
    }
  }

  items.forEach((item) => {
    const { w, h } = getGridSize(item)
    let candidate: [number, number] | undefined

    while (!candidate) {
      candidate = candidates.find(([centerX, centerY]) => {
        const gridX = centerX - Math.floor(w / 2)
        const gridY = centerY - Math.floor(h / 2)
        return fits(gridX, gridY, w, h)
      })
      if (!candidate) appendSquareRing(candidates, ++candidateRadius)
    }

    const gridX = candidate[0] - Math.floor(w / 2)
    const gridY = candidate[1] - Math.floor(h / 2)
    occupy(gridX, gridY, w, h)
    layouts.set(item.id, {
      left: gridX * LIBRARY_CANVAS_STRIDE - LIBRARY_CANVAS_CELL_SIZE / 2,
      top: gridY * LIBRARY_CANVAS_STRIDE - LIBRARY_CANVAS_CELL_SIZE / 2,
      width: w * LIBRARY_CANVAS_CELL_SIZE + (w - 1) * LIBRARY_CANVAS_GAP,
      height: h * LIBRARY_CANVAS_CELL_SIZE + (h - 1) * LIBRARY_CANVAS_GAP,
      gridX,
      gridY,
      gridW: w,
      gridH: h,
    })
  })

  const firstLayout = items.length > 0 ? layouts.get(items[0].id) : undefined
  if (firstLayout) {
    const anchorX = firstLayout.left + firstLayout.width / 2
    const anchorY = firstLayout.top + firstLayout.height / 2
    layouts.forEach((layout) => {
      const centerX = layout.left + layout.width / 2 - anchorX
      const centerY = layout.top + layout.height / 2 - anchorY
      layout.left = centerX * 1.32 - layout.width / 2
      layout.top = centerY * 1.32 - layout.height / 2
    })
  }

  return layouts
}

export function getLibraryCanvasFocusScaleAt(
  centerX: number,
  centerY: number,
  transform: LibraryCanvasTransform,
  viewport: LibraryCanvasViewport,
) {
  if (viewport.width <= 0 || viewport.height <= 0) {
    return LIBRARY_CANVAS_FOCUS_MAX_SCALE
  }
  const screenX = centerX * transform.scale + transform.x
  const screenY = centerY * transform.scale + transform.y
  const normalizedDistance = Math.hypot(
    screenX / (viewport.width / 2),
    screenY / (viewport.height / 2),
  )
  const progress = Math.min(1, normalizedDistance / LIBRARY_CANVAS_FOCUS_EXTENT)
  const easedProgress = progress * progress * (3 - 2 * progress)
  return (
    LIBRARY_CANVAS_FOCUS_MAX_SCALE -
    (LIBRARY_CANVAS_FOCUS_MAX_SCALE - LIBRARY_CANVAS_FOCUS_MIN_SCALE) *
      easedProgress
  )
}

export function getLibraryCanvasFocusScale(
  layout: LibraryCanvasLayout,
  transform: LibraryCanvasTransform,
  viewport: LibraryCanvasViewport,
) {
  return getLibraryCanvasFocusScaleAt(
    layout.left + layout.width / 2,
    layout.top + layout.height / 2,
    transform,
    viewport,
  )
}

export function getLibraryCanvasViewportBounds(
  transform: LibraryCanvasTransform,
  viewport: LibraryCanvasViewport,
  overscanPx = 360,
): LibraryCanvasBounds {
  const overscan = overscanPx / transform.scale
  return {
    minX: (-viewport.width / 2 - transform.x) / transform.scale - overscan,
    maxX: (viewport.width / 2 - transform.x) / transform.scale + overscan,
    minY: (-viewport.height / 2 - transform.y) / transform.scale - overscan,
    maxY: (viewport.height / 2 - transform.y) / transform.scale + overscan,
  }
}

export function getLibraryCanvasViewportBinKey(
  transform: LibraryCanvasTransform,
  viewport: LibraryCanvasViewport,
  binSize: number,
  overscanPx = 360,
): string {
  if (
    viewport.width <= 0 ||
    viewport.height <= 0 ||
    binSize <= 0 ||
    !Number.isFinite(binSize)
  ) {
    return '0'
  }
  const bounds = getLibraryCanvasViewportBounds(
    transform,
    viewport,
    overscanPx,
  )
  return [
    Math.floor(bounds.minX / binSize),
    Math.floor(bounds.maxX / binSize),
    Math.floor(bounds.minY / binSize),
    Math.floor(bounds.maxY / binSize),
  ].join(',')
}

export function libraryCanvasLayoutIntersects(
  layout: LibraryCanvasLayout,
  bounds: LibraryCanvasBounds,
) {
  return (
    layout.left + layout.width >= bounds.minX &&
    layout.left <= bounds.maxX &&
    layout.top + layout.height >= bounds.minY &&
    layout.top <= bounds.maxY
  )
}
