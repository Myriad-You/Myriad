export type IsoDate = string // YYYY-MM-DD

export function parseIso(iso: string): { y: number; m: number; d: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso.trim())
  if (!m) return null
  const y = Number(m[1])
  const mo = Number(m[2])
  const d = Number(m[3])
  if (!y || mo < 1 || mo > 12 || d < 1 || d > 31) return null
  return { y, m: mo, d }
}

export function toIso(y: number, m: number, d: number): IsoDate {
  return `${y}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`
}

export function daysInMonth(y: number, m: number): number {
  return new Date(y, m, 0).getDate()
}

export function weekdayMon0(y: number, m: number, d: number): number {
  const js = new Date(y, m - 1, d).getDay() // 0 Sun
  return (js + 6) % 7
}

export function addMonths(y: number, m: number, delta: number): { y: number; m: number } {
  const idx = y * 12 + (m - 1) + delta
  return { y: Math.floor(idx / 12), m: (idx % 12) + 1 }
}

export function inclusiveDaySpan(from: string, to: string): number {
  const a = parseIso(from)
  const b = parseIso(to)
  if (!a || !b) return 0
  const da = Date.UTC(a.y, a.m - 1, a.d)
  const db = Date.UTC(b.y, b.m - 1, b.d)
  return Math.floor(Math.abs(db - da) / 86_400_000) + 1
}

export type CalendarCell =
  | { kind: 'empty' }
  | {
      kind: 'day'
      iso: IsoDate
      day: number
      inMonth: boolean
      disabled: boolean
    }

export function buildMonthGrid(
  y: number,
  m: number,
  opts?: { min?: string; max?: string },
): CalendarCell[] {
  const dim = daysInMonth(y, m)
  const lead = weekdayMon0(y, m, 1)
  const cells: CalendarCell[] = []
  for (let i = 0; i < lead; i++) cells.push({ kind: 'empty' })
  for (let d = 1; d <= dim; d++) {
    const iso = toIso(y, m, d)
    const disabled =
      (!!opts?.min && iso < opts.min) || (!!opts?.max && iso > opts.max)
    cells.push({ kind: 'day', iso, day: d, inMonth: true, disabled })
  }
  while (cells.length % 7 !== 0) cells.push({ kind: 'empty' })
  while (cells.length < 42) cells.push({ kind: 'empty' })
  return cells
}

export type RangeDayState =
  | 'outside'
  | 'disabled'
  | 'plain'
  | 'start'
  | 'end'
  | 'single'
  | 'in-range'
  | 'hover-in'

export function rangeDayState(
  iso: string,
  opts: {
    from?: string
    to?: string
    hover?: string
    disabled?: boolean
  },
): RangeDayState {
  if (opts.disabled) return 'disabled'
  const from = opts.from
  const to = opts.to
  const hover = opts.hover

  if (from && to) {
    if (from === to && iso === from) return 'single'
    if (iso === from) return 'start'
    if (iso === to) return 'end'
    if (iso > from && iso < to) return 'in-range'
    return 'plain'
  }

  if (from && !to && hover) {
    const a = from <= hover ? from : hover
    const b = from <= hover ? hover : from
    if (a === b && iso === a) return 'single'
    if (iso === a) return 'start'
    if (iso === b) return 'end'
    if (iso > a && iso < b) return 'hover-in'
    return 'plain'
  }

  if (from && iso === from) return 'single'
  return 'plain'
}

export function applyRangeClick(
  iso: string,
  current: { from: string; to: string; picking: 'from' | 'to' },
): { from: string; to: string; picking: 'from' | 'to'; complete: boolean } {
  if (current.picking === 'from' || !current.from) {
    return { from: iso, to: '', picking: 'to', complete: false }
  }
  if (iso < current.from) {
    return { from: iso, to: current.from, picking: 'from', complete: true }
  }
  return { from: current.from, to: iso, picking: 'from', complete: true }
}
