/** 入口卡片自动收起。不写已完成，刷新或下次再进这一页还会出现。 */
export const TOUR_HINT_AUTO_HIDE_MS = 30_000

export function shouldAutoHideTourHint(
  dev: boolean = import.meta.env.DEV,
): boolean {
  return !dev
}
