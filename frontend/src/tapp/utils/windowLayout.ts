export interface WindowBounds {
  width: number
  height: number
}

export interface WindowRectangle {
  position: { x: number; y: number }
  size: WindowBounds
}

export const TAPP_DEFAULT_WINDOW_SIZE = { width: 400, height: 600 }

/** Bounds already exclude the dock; reserve space for the desktop controls. */
export function getTappWindowWorkArea(bounds: WindowBounds): WindowRectangle {
  const x = Math.min(16, bounds.width / 2)
  const y = Math.min(80, bounds.height)
  return {
    position: { x, y },
    size: {
      width: Math.max(0, bounds.width - x * 2),
      height: Math.max(0, bounds.height - y - 16),
    },
  }
}

/** Reserve at least two panes; choose rows that best fit the default window size. */
export function getTappWindowTiles(
  count: number,
  bounds: WindowBounds,
): WindowRectangle[] {
  const area = getTappWindowWorkArea(bounds)
  if (count < 1 || area.size.width <= 0 || area.size.height <= 0) return []

  // A single visible app occupies one half, leaving the other pane available.
  const paneCount = Math.max(2, count)
  const gap = 12
  let bestFit = -1
  let best: WindowRectangle[] = []
  for (let rows = 1; rows <= paneCount; rows++) {
    const height = (area.size.height - gap * (rows - 1)) / rows
    if (height <= 0) continue
    const tiles: WindowRectangle[] = []
    let fit = Infinity
    for (let row = 0; row < rows; row++) {
      const columns = Math.floor(paneCount / rows) + (row < paneCount % rows ? 1 : 0)
      const width = (area.size.width - gap * (columns - 1)) / columns
      if (width <= 0) {
        fit = -1
        break
      }
      fit = Math.min(
        fit,
        width / TAPP_DEFAULT_WINDOW_SIZE.width,
        height / TAPP_DEFAULT_WINDOW_SIZE.height,
      )
      for (let column = 0; column < columns; column++) {
        tiles.push({
          position: {
            x: area.position.x + column * (width + gap),
            y: area.position.y + row * (height + gap),
          },
          size: { width, height },
        })
      }
    }
    if (fit > bestFit) {
      bestFit = fit
      best = tiles
    }
  }
  return best.slice(0, count)
}
