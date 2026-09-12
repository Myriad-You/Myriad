export type SchedulePreset = '15m' | '30m' | '1h' | '6h' | 'daily9' | 'custom'

export const SCHEDULE_PRESETS: ReadonlyArray<{
  id: Exclude<SchedulePreset, 'custom'>
  cron: string
}> = [
  { id: '15m', cron: '*/15 * * * *' },
  { id: '30m', cron: '*/30 * * * *' },
  { id: '1h', cron: '0 * * * *' },
  { id: '6h', cron: '0 */6 * * *' },
  { id: 'daily9', cron: '0 9 * * *' },
]

export function matchSchedulePreset(cron: string): SchedulePreset {
  const raw = cron.trim()
  return SCHEDULE_PRESETS.find((preset) => preset.cron === raw)?.id ?? 'custom'
}

export type ScheduleShape =
  | { kind: 'everyMinutes'; value: number }
  | { kind: 'everyHours'; value: number }
  | { kind: 'hourly' }
  | { kind: 'dailyAt'; time: string }
  | { kind: 'raw'; cron: string }

const EVERY_MINUTES = /^\*\/(\d+)\s+\*\s+\*\s+\*\s+\*$/
const EVERY_HOURS = /^0\s+\*\/(\d+)\s+\*\s+\*\s+\*$/

function isWildcard(field: string | undefined): boolean {
  return field === '*' || field === '?'
}

export function describeSchedule(cron: string): ScheduleShape {
  const raw = cron.trim()

  const minutes = raw.match(EVERY_MINUTES)
  if (minutes) {
    const value = Number(minutes[1])
    if (value > 0) return { kind: 'everyMinutes', value }
  }

  const hours = raw.match(EVERY_HOURS)
  if (hours) {
    const value = Number(hours[1])
    if (value > 0) return { kind: 'everyHours', value }
  }

  if (raw === '0 * * * *') return { kind: 'hourly' }

  const parts = raw.split(/\s+/)
  if (parts.length >= 5) {
    const [minute, hour, dayOfMonth, month, dayOfWeek] = parts
    const fixed =
      hour &&
      minute &&
      hour !== '*' &&
      minute !== '*' &&
      !hour.includes('/') &&
      !minute.includes('/') &&
      !hour.includes(',') &&
      !minute.includes(',') &&
      !hour.includes('-') &&
      !minute.includes('-')
    if (
      fixed &&
      isWildcard(dayOfMonth) &&
      isWildcard(month) &&
      isWildcard(dayOfWeek)
    ) {
      return {
        kind: 'dailyAt',
        time: `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}`,
      }
    }
  }

  return { kind: 'raw', cron: raw }
}

export function isPlausibleCron(cron: string): boolean {
  const parts = cron.trim().split(/\s+/).filter(Boolean)
  return parts.length === 5
}
