/** 定时发布的快捷时间和倒计时文字。只算时间，不碰界面。 */

export type NoteSchedulePresetKey = 'hour' | 'tonight' | 'tomorrowMorning' | 'tomorrowEvening'

const MINUTE = 60_000
const HOUR = 60 * MINUTE

function atLocal(base: number, dayOffset: number, hour: number): number {
  const date = new Date(base)
  date.setDate(date.getDate() + dayOffset)
  date.setHours(hour, 0, 0, 0)
  return date.getTime()
}

/**
 * 三个最常用的去处：一小时后（取整到 5 分钟）、晚上 8 点、明早 9 点。
 * 今晚 8 点离现在不到一小时就改成明晚，免得和「一小时后」挤在一起。按时间先后排。
 */
export function noteSchedulePresets(now: number): Array<{ key: NoteSchedulePresetKey; at: number }> {
  const hour = Math.ceil((now + HOUR) / (5 * MINUTE)) * 5 * MINUTE
  const tonight = atLocal(now, 0, 20)
  const evening =
    tonight - now >= HOUR
      ? { key: 'tonight' as const, at: tonight }
      : { key: 'tomorrowEvening' as const, at: atLocal(now, 1, 20) }
  return [
    { key: 'hour' as const, at: hour },
    evening,
    { key: 'tomorrowMorning' as const, at: atLocal(now, 1, 9) },
  ].sort((a, b) => a.at - b.at)
}

/** 「23 小时后」「3 天后」：不到一小时按分钟，不到两天按小时，再远按天。 */
export function noteRelativeUntil(at: number, now: number, locale: string): string {
  const diff = Math.max(0, at - now)
  const format = new Intl.RelativeTimeFormat(locale, { numeric: 'always' })
  if (diff < HOUR) return format.format(Math.max(1, Math.ceil(diff / MINUTE)), 'minute')
  if (diff < 48 * HOUR) return format.format(Math.round(diff / HOUR), 'hour')
  return format.format(Math.round(diff / (24 * HOUR)), 'day')
}
