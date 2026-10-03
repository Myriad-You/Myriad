/**
 * 首页报告卡的分页：总览是第 0 页，详情里的每一项（或每一对）各占一页。
 * 平台面上报「详情有几页、每页停多久」，报告卡统一掌管当前页，页码点和自动翻页都按真实页数走。
 *
 * 只有首页可交互的报告卡挂 Provider；报告页、舞台里嵌着的卡片没有 Provider，
 * 平台面拿到 null，照旧自己轮换。
 */

import { createContext, useContext, useLayoutEffect } from 'react'

export interface ReportDetailPaging {
  /** 当前详情页（0 起）；在总览时为 0，平台面不该用它决定是否显示详情。 */
  detailIndex: number
  /** 平台面上报详情页数与每页停留时长（ms）。 */
  register: (pages: number, dwellMs: number) => void
}

export const ReportDetailPagingContext =
  createContext<ReportDetailPaging | null>(null)

/** 一页一项的详情（作品、推文、服务器）每页停留。 */
export const REPORT_ITEM_DWELL_MS = 6000
/** 一页两项的详情（番剧、歌曲）每页停留。 */
export const REPORT_PAIR_DWELL_MS = 5000

/**
 * 平台面在每次渲染都调用（总览时也要），返回由报告卡掌管的详情页；
 * 没有 Provider 时返回 null，调用方用自己的轮换。
 */
export function useReportDetailPage(
  pages: number,
  dwellMs: number,
): number | null {
  const paging = useContext(ReportDetailPagingContext)
  useLayoutEffect(() => {
    if (!paging) return
    paging.register(pages, dwellMs)
    // 换平台后新的平台面可能不上报（详情不分页），旧页数不能留着。
    return () => paging.register(0, dwellMs)
  }, [paging, pages, dwellMs])
  if (!paging) return null
  return pages > 0 ? paging.detailIndex % pages : 0
}

/** 一页两项时的页数。 */
export function pairPageCount(items: number): number {
  return Math.ceil(items / 2)
}
