/**
 * Widget load performance marks for multi-widget Dashboard measurement.
 *
 * Uses the User Timing API (`performance.mark` / `measure`) so LCP/TTI-adjacent
 * work can be inspected in DevTools or via `window.__MYRIAD_TAPP_WIDGET_PERF__`.
 *
 * This does not change security boundaries; marks are host-side only.
 */

export type WidgetPerfPhase =
  | 'host-load-start'
  | 'resources-ready'
  | 'sandbox-mount'
  | 'iframe-ready'

export interface WidgetPerfRecord {
  tappId: string
  widgetId: string
  instanceKey: string
  marks: Partial<Record<WidgetPerfPhase, number>>
  measures: {
    hostToResourcesMs?: number
    resourcesToMountMs?: number
    mountToReadyMs?: number
    totalHostToReadyMs?: number
  }
}

const PHASES: WidgetPerfPhase[] = [
  'host-load-start',
  'resources-ready',
  'sandbox-mount',
  'iframe-ready',
]

const records = new Map<string, WidgetPerfRecord>()
const MAX_RECORDS = 80

function now(): number {
  return typeof performance !== 'undefined' && typeof performance.now === 'function'
    ? performance.now()
    : Date.now()
}

function markName(instanceKey: string, phase: WidgetPerfPhase): string {
  return `tapp-widget:${instanceKey}:${phase}`
}

function safeMark(name: string): void {
  try {
    if (typeof performance !== 'undefined' && typeof performance.mark === 'function') {
      performance.mark(name)
    }
  } catch {
    // User Timing can throw if the name collides in some engines; ignore.
  }
}

function safeMeasure(
  name: string,
  startMark: string,
  endMark: string,
): number | undefined {
  try {
    if (
      typeof performance === 'undefined' ||
      typeof performance.measure !== 'function'
    ) {
      return undefined
    }
    performance.measure(name, startMark, endMark)
    const entries = performance.getEntriesByName(name, 'measure')
    const last = entries[entries.length - 1]
    return last?.duration
  } catch {
    return undefined
  }
}

export function widgetInstanceKey(
  tappId: string,
  widgetId: string,
  size?: string,
): string {
  return size ? `${tappId}/${widgetId}@${size}` : `${tappId}/${widgetId}`
}

export function widgetPerfMark(
  tappId: string,
  widgetId: string,
  phase: WidgetPerfPhase,
  size?: string,
): void {
  const instanceKey = widgetInstanceKey(tappId, widgetId, size)
  let record = records.get(instanceKey)
  if (!record) {
    record = {
      tappId,
      widgetId,
      instanceKey,
      marks: {},
      measures: {},
    }
    records.set(instanceKey, record)
    while (records.size > MAX_RECORDS) {
      const oldest = records.keys().next().value
      if (oldest === undefined || oldest === instanceKey) break
      records.delete(oldest)
    }
  } else {
    records.delete(instanceKey)
    records.set(instanceKey, record)
  }
  const t = now()
  record.marks[phase] = t
  safeMark(markName(instanceKey, phase))

  // Progressive measures as soon as endpoints exist
  const m = record.measures
  const marks = record.marks
  if (
    phase === 'resources-ready' &&
    marks['host-load-start'] !== undefined
  ) {
    m.hostToResourcesMs = t - marks['host-load-start']
    safeMeasure(
      `tapp-widget:${instanceKey}:host-to-resources`,
      markName(instanceKey, 'host-load-start'),
      markName(instanceKey, 'resources-ready'),
    )
  }
  if (phase === 'sandbox-mount' && marks['resources-ready'] !== undefined) {
    m.resourcesToMountMs = t - marks['resources-ready']
    safeMeasure(
      `tapp-widget:${instanceKey}:resources-to-mount`,
      markName(instanceKey, 'resources-ready'),
      markName(instanceKey, 'sandbox-mount'),
    )
  }
  if (phase === 'iframe-ready' && marks['sandbox-mount'] !== undefined) {
    m.mountToReadyMs = t - marks['sandbox-mount']
    safeMeasure(
      `tapp-widget:${instanceKey}:mount-to-ready`,
      markName(instanceKey, 'sandbox-mount'),
      markName(instanceKey, 'iframe-ready'),
    )
  }
  if (phase === 'iframe-ready' && marks['host-load-start'] !== undefined) {
    m.totalHostToReadyMs = t - marks['host-load-start']
    safeMeasure(
      `tapp-widget:${instanceKey}:total-host-to-ready`,
      markName(instanceKey, 'host-load-start'),
      markName(instanceKey, 'iframe-ready'),
    )
  }
}

export function getWidgetPerfSnapshot(): WidgetPerfRecord[] {
  return [...records.values()].map((r) => ({
    ...r,
    marks: { ...r.marks },
    measures: { ...r.measures },
  }))
}

export function getWidgetPerfSummary(): {
  count: number
  readyCount: number
  avgTotalHostToReadyMs: number | null
  maxTotalHostToReadyMs: number | null
  p95TotalHostToReadyMs: number | null
  records: WidgetPerfRecord[]
} {
  const all = getWidgetPerfSnapshot()
  const totals = all
    .map((r) => r.measures.totalHostToReadyMs)
    .filter((n): n is number => typeof n === 'number' && Number.isFinite(n))
    .sort((a, b) => a - b)
  const readyCount = totals.length
  const avg =
    readyCount > 0
      ? totals.reduce((s, n) => s + n, 0) / readyCount
      : null
  const max = readyCount > 0 ? totals[totals.length - 1]! : null
  const p95 =
    readyCount > 0
      ? totals[Math.min(readyCount - 1, Math.floor(readyCount * 0.95))]!
      : null
  return {
    count: all.length,
    readyCount,
    avgTotalHostToReadyMs: avg,
    maxTotalHostToReadyMs: max,
    p95TotalHostToReadyMs: p95,
    records: all,
  }
}

export function clearWidgetPerf(): void {
  const previous = [...records.values()]
  records.clear()
  try {
    if (
      typeof performance !== 'undefined' &&
      typeof performance.clearMarks === 'function'
    ) {
      for (const record of previous) {
        for (const phase of PHASES) {
          performance.clearMarks(markName(record.instanceKey, phase))
        }
      }
    }
  } catch {
    // ignore
  }
}

/** DevTools helper — available after first import of widget runtime. */
export function installWidgetPerfGlobal(): void {
  if (typeof window === 'undefined') return
  const target = window as Window & {
    __MYRIAD_TAPP_WIDGET_PERF__?: {
      getSnapshot: typeof getWidgetPerfSnapshot
      getSummary: typeof getWidgetPerfSummary
      clear: typeof clearWidgetPerf
      mark: typeof widgetPerfMark
    }
  }
  target.__MYRIAD_TAPP_WIDGET_PERF__ = {
    getSnapshot: getWidgetPerfSnapshot,
    getSummary: getWidgetPerfSummary,
    clear: clearWidgetPerf,
    mark: widgetPerfMark,
  }
}

// Install eagerly in browser so Dashboard multi-widget sessions can measure
// without importing this module from the console.
installWidgetPerfGlobal()
