/**
 * 控制面板小组件区的行数切换重排。
 *
 * 背景：切换 1↔2 行时会改变每个小组件的尺寸（1 行模式统一压成 4x1），
 * 但旧实现只改 size、保留原 x，宽度翻倍而横坐标不动 —— 相邻小组件必然叠上。
 * 默认布局的两个 2x2 就在 x=0 / x=2，切到 1 行后占 [0,4) 与 [2,6)，重叠两列。
 * 更糟的是 y 被强制归 0 且不可逆，切回 2 行时不恢复位置，于是原本上下排布的
 * 小组件全挤在 y=0 再次重叠，而这个坏掉的布局会被持久化到后端。
 *
 * 这里改为每次切换都按当前顺序重新打包，保证输出恒不重叠。
 */

/** 每页列数；一页正好是分页滑动的一屏。 */
export const PAGE_COLS = 4
/** 页数；与 WidgetGrid 的 customGridColumns={12} 对应（4 × 3）。 */
export const PAGE_COUNT = 3

export interface GridSpan {
  w: number
  h: number
}

/** 解析 `"4x1"` 形式的尺寸；无法解析或非正数返回 null。 */
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

/**
 * 把小组件按当前顺序重新打包进 PAGE_COUNT 页 × PAGE_COLS 列 × rows 行的网格。
 *
 * - 页内首次适配，先填满一行再换行，填不下就进下一页
 * - 不跨页：横跨页边界的小组件会被翻页滑动切开
 * - 放不下的（高于 rows、宽于一页、或格子已满）直接丢弃
 *
 * 输出保证任意两个小组件的占用矩形互不相交。
 */
export function packControlPanelWidgets<T extends Placeable>(
  widgets: readonly T[],
  rows: number,
): T[] {
  if (rows <= 0) return []

  // occupied[page][row][col]
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

  const out: T[] = []
  for (const widget of widgets) {
    const span = parseWidgetSize(widget.size)
    // 高于当前行数或宽于一页 —— 在这个网格里无处安放
    if (!span || span.h > rows || span.w > PAGE_COLS) continue

    let placed = false
    for (let p = 0; p < PAGE_COUNT && !placed; p++) {
      for (let y = 0; y + span.h <= rows && !placed; y++) {
        for (let x = 0; x + span.w <= PAGE_COLS && !placed; x++) {
          if (!fits(occupied[p], x, y, span)) continue
          occupy(occupied[p], x, y, span)
          out.push({ ...widget, position: { x: p * PAGE_COLS + x, y } })
          placed = true
        }
      }
    }
  }
  return out
}

/** 两个小组件的占用矩形是否相交 —— 供测试与 DEV 断言使用。 */
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
