// 打包是渲染规范化不是数据变换；装不下的必须原样回传，否则改行数会永久删掉格子。

export const PAGE_COLS = 4
export const PAGE_COUNT = 3

export interface GridSpan {
  w: number
  h: number
}

export function parseWidgetSize(size: string): GridSpan | null {
  const m = /^(\d+)x(\d+)$/.exec(size)
  if (!m) return null
  const w = Number(m[1])
  const h = Number(m[2])
  if (w <= 0 || h <= 0) return null
  return { w, h }
}

interface Placeable {
  size: string
  position: { x: number, y: number }
}

export interface PackResult<T> {
  placed: T[]
  // overflow 原样保留并与 placed 一起持久化，换回更大行数时再放回去。
  overflow: T[]
}

export function packControlPanelWidgets<T extends Placeable>(
  widgets: readonly T[],
  rows: number,
): PackResult<T> {
  if (rows <= 0) return { placed: [], overflow: Iterator.from(widgets).toArray() }

  const occupied: boolean[][][] = Array.from({ length: PAGE_COUNT }, () =>
    Array.from({ length: rows }, () =>
      Array.from({ length: PAGE_COLS }, () => false),
    ),
  )

  const fits = (page: boolean[][], x: number, y: number, span: GridSpan) => {
    for (let dy = 0; dy < span.h; dy++) {
      for (let dx = 0; dx < span.w; dx++) {
        if (page[y + dy][x + dx]) return false
      }
    }
    return true
  }

  const occupy = (page: boolean[][], x: number, y: number, span: GridSpan) => {
    for (let dy = 0; dy < span.h; dy++) {
      for (let dx = 0; dx < span.w; dx++) {
        page[y + dy][x + dx] = true
      }
    }
  }

  const placed: T[] = []
  const overflow: T[] = []
  for (const widget of widgets) {
    const span = parseWidgetSize(widget.size)
    if (!span || span.h > rows || span.w > PAGE_COLS) {
      overflow.push(widget)
      continue
    }

    let done = false
    for (let p = 0; p < PAGE_COUNT && !done; p++) {
      for (let y = 0; y + span.h <= rows && !done; y++) {
        for (let x = 0; x + span.w <= PAGE_COLS && !done; x++) {
          if (!fits(occupied[p], x, y, span)) continue
          occupy(occupied[p], x, y, span)
          placed.push({ ...widget, position: { x: p * PAGE_COLS + x, y } })
          done = true
        }
      }
    }
    if (!done) overflow.push(widget)
  }
  return { placed, overflow }
}

// 编辑时必须把 overflow 并回去，否则 1 行模式一拖就从存盘删掉。
export function mergeVisibleWithHidden<T extends { id: string }>(
  visible: readonly T[],
  hidden: readonly T[],
): T[] {
  if (hidden.length === 0) return Iterator.from(visible).toArray()
  const visibleIds = new Set(visible.map((w) => w.id))
  const stillHidden = Iterator.from(hidden)
    .filter((w) => !visibleIds.has(w.id))
    .toArray()
  return stillHidden.length > 0 ? [...visible, ...stillHidden] : Iterator.from(visible).toArray()
}

export function widgetsOverlap(a: Placeable, b: Placeable): boolean {
  const sa = parseWidgetSize(a.size)
  const sb = parseWidgetSize(b.size)
  if (!sa || !sb) return false
  return (
    a.position.x < b.position.x + sb.w &&
    b.position.x < a.position.x + sa.w &&
    a.position.y < b.position.y + sb.h &&
    b.position.y < a.position.y + sa.h
  )
}
