/**
 * 伴读胶囊摆在哪：照改版前阅读器的两侧浮动——左右各一个竖胶囊，贴着正文两边、留 32px，
 * 竖直居中。主题千差万别，正文在哪只能量；左边还要让开主站的导航岛。两边放不下
 * （手机、窄窗口）就换成底部居中的一条，手机上导航岛横在底部，就叠在它上面。
 */

export interface Box {
  left: number
  right: number
  top: number
  bottom: number
}

export type CompanionLayout =
  | { mode: 'sides'; left: number; right: number }
  | { mode: 'bar'; bottom: number }

/** 胶囊外宽：36px 的按钮加 8px 内边距和 1px 边框。 */
export const CAPSULE_WIDTH = 54
export const CAPSULE_GAP = 32
const EDGE = 12
/** 比这窄就不放两侧，和旧阅读器的 `sm` 一样。 */
const SIDES_MIN_WIDTH = 640
const BAR_BOTTOM = 18

export function companionLayout(
  view: { width: number; height: number },
  article: Box,
  nav: Box | null,
): CompanionLayout {
  const navVertical = nav != null && nav.bottom - nav.top > nav.right - nav.left
  const navAtBottom = nav != null && !navVertical && nav.bottom > view.height - 120
  const left = article.left - CAPSULE_GAP - CAPSULE_WIDTH
  const right = article.right + CAPSULE_GAP
  const leftLimit = (navVertical ? nav.right : 0) + EDGE
  if (view.width >= SIDES_MIN_WIDTH && left >= leftLimit && right + CAPSULE_WIDTH <= view.width - EDGE) {
    return { mode: 'sides', left: Math.round(left), right: Math.round(right) }
  }
  return {
    mode: 'bar',
    bottom: navAtBottom ? Math.round(view.height - nav.top + EDGE) : BAR_BOTTOM,
  }
}

/** 读到哪了：正文顶端过了视口顶端算起，正文底端到了视口底端算读完。 */
export function readingProgress(article: Box, viewHeight: number): number {
  const span = article.bottom - article.top - viewHeight
  if (span <= 0) return article.top < viewHeight ? 1 : 0
  return Math.min(1, Math.max(0, -article.top / span))
}

export interface ReadingPrefs {
  /** 正文字号，百分比；null 是主题自己的。 */
  size: number | null
  /** 行距倍数；null 是主题自己的。 */
  leading: number | null
}

export const SIZE_STEPS = [85, 90, 95, 100, 110, 120, 135] as const
export const LEADING_STEPS = [1.4, 1.5, 1.6, 1.75, 1.9, 2.1] as const

/** 从眼下的值往大或往小挪一档；到头了就不动。 */
export function stepFrom(steps: readonly number[], current: number, direction: 1 | -1): number {
  if (direction > 0) return steps.find((step) => step > current + 1e-6) ?? steps.at(-1)!
  return steps.findLast((step) => step < current - 1e-6) ?? steps[0]
}

export function readingPrefsFromValue(value: unknown): ReadingPrefs {
  const record = value && typeof value === 'object' ? (value as Record<string, unknown>) : {}
  const pick = (raw: unknown, steps: readonly number[]) =>
    typeof raw === 'number' && steps.includes(raw) ? raw : null
  return { size: pick(record.size, SIZE_STEPS), leading: pick(record.leading, LEADING_STEPS) }
}
