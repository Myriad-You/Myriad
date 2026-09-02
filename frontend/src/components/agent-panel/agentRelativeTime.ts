/**
 * 「刚刚 / 5 分钟前 / 3 小时前 / 2 天前」。
 *
 * 只算档位，不拼字 —— 拼字要按语言走，交给 i18n。分开之后这一步能脱离浏览器测，
 * 而它恰恰是容易出错的一步：时区、未来时间、刚好卡在整点的边界。
 */

export type RelativeTimeBucket =
  | { kind: 'justNow' }
  | { kind: 'minutes'; value: number }
  | { kind: 'hours'; value: number }
  | { kind: 'days'; value: number }
  /** 太久远就报日期，别让人算「43 天前」是哪天 */
  | { kind: 'date'; date: Date }

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

/** 超过这个天数改报日期。 */
export const RELATIVE_TIME_MAX_DAYS = 7

export function relativeTimeBucket(
  iso: string | null | undefined,
  nowMs: number,
): RelativeTimeBucket | null {
  if (!iso) return null
  const then = new Date(iso)
  const at = then.getTime()
  if (!Number.isFinite(at)) return null

  // 服务端时钟稍快就会算出负数。未来的时间当「刚刚」，不显示「-1 分钟前」。
  const elapsed = Math.max(0, nowMs - at)

  if (elapsed < MINUTE) return { kind: 'justNow' }
  if (elapsed < HOUR) {
    return { kind: 'minutes', value: Math.floor(elapsed / MINUTE) }
  }
  if (elapsed < DAY) return { kind: 'hours', value: Math.floor(elapsed / HOUR) }
  const days = Math.floor(elapsed / DAY)
  if (days <= RELATIVE_TIME_MAX_DAYS) return { kind: 'days', value: days }
  return { kind: 'date', date: then }
}
