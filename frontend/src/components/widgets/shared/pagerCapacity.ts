/** 一个点连同左右留白的宽度（宽松点距）。 */
const DOT_SLOT_PX = 11
/** 当前页那颗拉长的点多出来的宽度、胶囊内边距，以及离卡片左右边缘的余量。 */
const PAGER_CHROME_PX = 32
/** 暂停/继续按钮占的宽度。 */
export const TOGGLE_PX = 20

/** 给定可用宽度时，用点最多能排下几页；排不下就该改用「‹ 2 / 24 ›」计数器。 */
export function pagerDotCapacity(availablePx: number): number {
  return Math.max(0, Math.floor((availablePx - PAGER_CHROME_PX) / DOT_SLOT_PX))
}
