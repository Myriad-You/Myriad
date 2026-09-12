/** auto: may drop tier, never raise; persist in localStorage. */

const STORAGE_KEY = 'myriad-anim-auto-want-high'

const IDLE_BEFORE_SAMPLE_MS = 2500
const SAMPLE_GAPS = 24
const BAD_FRAME_MS = 34
const SEVERE_FRAME_MS = 50
const BAD_RATIO_THRESHOLD = 0.45
const MIN_SEVERE_FRAMES = 4
const AVG_FRAME_MS_THRESHOLD = 24
const MAX_GAP_MS = 120

export interface AutoSampleResult {
  demoted: boolean
  avgMs: number
  badRatio: number
  severeCount: number
  frames: number
}

export function getStoredAutoWantHigh(): boolean {
  if (typeof localStorage === 'undefined') return true
  try {
    const v = localStorage.getItem(STORAGE_KEY)
    if (v === '0') return false
    if (v === '1') return true
  } catch {
    /* private mode */
  }
  return true
}

export function setStoredAutoWantHigh(wantHigh: boolean): void {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, wantHigh ? '1' : '0')
  } catch {
    /* ignore */
  }
}

export function clearAutoDemoteMemory(): void {
  setStoredAutoWantHigh(true)
  probeStarted = false
  activeCancel?.()
  activeCancel = null
}

export function evaluateCounters(
  frames: number,
  sumMs: number,
  bad: number,
  severe: number,
): AutoSampleResult {
  if (frames < 12) {
    return {
      demoted: false,
      avgMs: 0,
      badRatio: 0,
      severeCount: severe,
      frames,
    }
  }
  const avgMs = sumMs / frames
  const badRatio = bad / frames
  const demoted =
    badRatio >= BAD_RATIO_THRESHOLD &&
    severe >= MIN_SEVERE_FRAMES &&
    avgMs >= AVG_FRAME_MS_THRESHOLD
  return { demoted, avgMs, badRatio, severeCount: severe, frames }
}

let probeStarted = false
let activeCancel: (() => void) | null = null
const demoteListeners = new Set<(result: AutoSampleResult) => void>()

export function startAutoFrameAdapt(options?: {
  onDemote?: (result: AutoSampleResult) => void
  enabled?: boolean
}): () => void {
  if (options?.enabled === false || typeof window === 'undefined') {
    return () => {}
  }

  const onDemote = options?.onDemote
  if (onDemote) demoteListeners.add(onDemote)

  const unsubscribe = () => {
    if (onDemote) demoteListeners.delete(onDemote)
  }

  // Skip probe if low tier is remembered.
  if (!getStoredAutoWantHigh()) {
    return unsubscribe
  }

  if (probeStarted) {
    return unsubscribe
  }

  probeStarted = true
  activeCancel = runProbeOnce((result) => {
    activeCancel = null
    // One probe per page.
    if (!result.demoted) return
    if (!getStoredAutoWantHigh()) return
    setStoredAutoWantHigh(false)
    for (const fn of demoteListeners) {
      try {
        fn(result)
      } catch {
        /* ignore */
      }
    }
  })

  return unsubscribe
}

function runProbeOnce(onDone: (result: AutoSampleResult) => void): () => void {
  let cancelled = false
  let idleTimer: ReturnType<typeof setTimeout> | null = null
  let rafId = 0
  let idleCallbackId = 0
  let visHandler: (() => void) | null = null
  let done = false

  const cleanupTimers = () => {
    if (idleTimer != null) {
      clearTimeout(idleTimer)
      idleTimer = null
    }
    if (rafId) {
      cancelAnimationFrame(rafId)
      rafId = 0
    }
    if (idleCallbackId && 'cancelIdleCallback' in window) {
      ;(
        window as Window & { cancelIdleCallback: (id: number) => void }
      ).cancelIdleCallback(idleCallbackId)
      idleCallbackId = 0
    }
    if (visHandler) {
      document.removeEventListener('visibilitychange', visHandler)
      visHandler = null
    }
  }

  const finish = (result: AutoSampleResult) => {
    if (done) return
    done = true
    cancelled = true
    cleanupTimers()
    onDone(result)
  }

  const runSample = () => {
    if (cancelled || document.hidden) return

    let last = 0
    let gaps = 0
    let sumMs = 0
    let bad = 0
    let severe = 0
    let sawFirst = false

    const tick = (now: number) => {
      if (cancelled) return

      if (!sawFirst) {
        sawFirst = true
        last = now
        rafId = requestAnimationFrame(tick)
        return
      }

      const dt = now - last
      last = now

      if (dt > 0 && dt < MAX_GAP_MS) {
        gaps += 1
        sumMs += dt
        if (dt >= BAD_FRAME_MS) {
          bad += 1
          if (dt >= SEVERE_FRAME_MS) severe += 1
        }
      }

      if (gaps < SAMPLE_GAPS) {
        rafId = requestAnimationFrame(tick)
        return
      }

      rafId = 0
      finish(evaluateCounters(gaps, sumMs, bad, severe))
    }

    rafId = requestAnimationFrame(tick)
  }

  const schedule = () => {
    if (cancelled) return
    if (document.hidden) {
      visHandler = () => {
        if (document.hidden || cancelled) return
        if (visHandler) {
          document.removeEventListener('visibilitychange', visHandler)
          visHandler = null
        }
        schedule()
      }
      document.addEventListener('visibilitychange', visHandler)
      return
    }

    const kick = () => {
      if (cancelled) return
      idleTimer = setTimeout(runSample, IDLE_BEFORE_SAMPLE_MS)
    }

    const w = window as Window & {
      requestIdleCallback?: (
        cb: () => void,
        opts?: { timeout: number },
      ) => number
    }
    if (typeof w.requestIdleCallback === 'function') {
      idleCallbackId = w.requestIdleCallback(kick, { timeout: 5000 })
    } else {
      kick()
    }
  }

  schedule()

  return () => {
    if (done) return
    cancelled = true
    cleanupTimers()
  }
}

export function __evaluateSampleForTest(intervals: number[]): AutoSampleResult {
  let sum = 0
  let bad = 0
  let severe = 0
  let n = 0
  for (let i = 0; i < intervals.length; i++) {
    const dt = intervals[i]
    if (dt <= 0 || dt >= MAX_GAP_MS) continue
    n += 1
    sum += dt
    if (dt >= BAD_FRAME_MS) {
      bad += 1
      if (dt >= SEVERE_FRAME_MS) severe += 1
    }
  }
  return evaluateCounters(n, sum, bad, severe)
}
