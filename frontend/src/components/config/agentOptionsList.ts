/**
 * Agent 设置三条列表（定时 / 技能 / 记忆）的显示窗口。
 *
 * 短列表跟着内容长，不套滚动条。超过视口阈值就进滚动，
 * 首批挂载条数对齐统计排行 RankList（先 30，其余「显示更多」）。
 */

/** 超过这个条数才进滚动窗口。 */
export const AGENT_OPTIONS_LIST_SCROLL_AFTER = 8

/** 滚动窗口里首批挂载条数。 */
export const AGENT_OPTIONS_LIST_PAGE = 30

export function agentOptionsListWindow(count: number): {
  maxHeight: '22rem' | null
  maxVisibleItems: number | null
} {
  if (count > AGENT_OPTIONS_LIST_SCROLL_AFTER) {
    return { maxHeight: '22rem', maxVisibleItems: AGENT_OPTIONS_LIST_PAGE }
  }
  return { maxHeight: null, maxVisibleItems: null }
}
