const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

/**
 * 评论上的时间：一分钟内「刚刚」，一周内「3 小时前」「2 天前」，再早写日期（今年的不写年）。
 * 完整时间放在 title 里。
 */
export function commentWhen(at: number, now: number, locale: string, justNow: string): string {
  const diff = now - at
  if (diff < MINUTE) return justNow
  const rel = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  if (diff < HOUR) return rel.format(-Math.floor(diff / MINUTE), 'minute')
  if (diff < DAY) return rel.format(-Math.floor(diff / HOUR), 'hour')
  if (diff < 7 * DAY) return rel.format(-Math.floor(diff / DAY), 'day')
  const date = new Date(at)
  const sameYear = date.getFullYear() === new Date(now).getFullYear()
  return date.toLocaleDateString(locale, sameYear ? { month: 'short', day: 'numeric' } : { dateStyle: 'medium' })
}

/** 没头像时的底色：同一个人每次一样。 */
export function commentHue(seed: string | number): number {
  const text = String(seed)
  let hash = 0
  for (let i = 0; i < text.length; i++) hash = (hash * 31 + text.charCodeAt(i)) | 0
  // 黄金角散开：相邻的 id（1、2、3）也落在差得远的色相上。
  return Math.round((Math.abs(hash) * 137.508) % 360)
}
