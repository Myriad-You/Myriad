import type { CSSProperties } from 'react'
import {

  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import './LyricWaveScroll.css'

export interface LyricWaveLine { time: number; text: string }

export type LyricWaveVariant = 'card' | 'panel'

export interface LyricWaveScrollProps {
  lyrics: LyricWaveLine[]
  currentLyricIndex: number
  variant?: LyricWaveVariant
  musicColor?: string
  className?: string
  paused?: boolean
}

const FOCAL = 0.5
const SCALE_ACTIVE = 1
const SCALE_INACTIVE = 0.78
const WAVE_DELAY = 40
const WAVE_SPAN = 5
const SEEK_JUMP = 3
const WINDOW = 12
const WIN_MARGIN = 4
const EST_LINE_H = 18
const K = 150
const C = 2 * Math.sqrt(K)
const KS = 240
const CS = 2 * Math.sqrt(KS)

function clampFocusS(
  lineY: number,
  lineH: number,
  viewH: number,
  desiredS: number,
): number {
  if (viewH < 8) return desiredS
  const visualH = lineH * SCALE_ACTIVE
  const pad = 2
  const minS = lineY + visualH - viewH + pad
  const maxS = lineY - pad
  if (minS > maxS) {
    return lineY + visualH / 2 - viewH / 2
  }
  return Math.max(minS, Math.min(maxS, desiredS))
}

function clampActivePos(
  pos: number,
  lineH: number,
  scale: number,
  viewH: number,
): number {
  if (viewH < 8) return pos
  const visualH = lineH * Math.max(0.01, scale)
  const pad = 2
  const minPos = pad
  const maxPos = viewH - visualH - pad
  if (maxPos < minPos) {
    return (viewH - visualH) / 2
  }
  return Math.max(minPos, Math.min(maxPos, pos))
}

interface WaveItem {
  idx: number
  text: string
  el: HTMLElement | null
  y: number
  h: number
  pos: number
  v: number
  scale: number
  scaleV: number
  targetScale: number
  delayUntil: number
  _wy: number
  _ws: number
}

function preferReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function rowClass(i: number, focus: number): string {
  const d = Math.abs(i - focus)
  let c = 'lyric-wave__row'
  if (i === focus) c += ' is-active'
  else if (d === 1) c += ' is-near-1'
  else if (d === 2) c += ' is-near-2'
  else c += ' is-far'
  if (i < focus) c += ' is-passed'
  return c
}

function cleanLyricLines(lyrics: LyricWaveLine[]) {
  const out: { text: string; origIdx: number }[] = []
  for (let i = 0; i < lyrics.length; i++) {
    const text = (lyrics[i]?.text || '').trim()
    if (text) out.push({ text, origIdx: i })
  }
  return out
}

function mapFocusIdx(
  clean: { origIdx: number }[],
  currentLyricIndex: number,
): number {
  if (clean.length === 0) return -1
  if (currentLyricIndex < 0) return 0
  let best = 0
  for (let i = 0; i < clean.length; i++) {
    if (clean[i].origIdx <= currentLyricIndex) best = i
    else break
  }
  return best
}

export const LyricWaveScroll = memo(({
  lyrics,
  currentLyricIndex,
  variant = 'card',
  musicColor = '#ef4444',
  className,
  paused = false,
}: LyricWaveScrollProps) => {
  const cleanLines = useMemo(() => cleanLyricLines(lyrics), [lyrics])
  const focusIdx = useMemo(
    () => mapFocusIdx(cleanLines, currentLyricIndex),
    [cleanLines, currentLyricIndex],
  )

  const computeWinRange = useCallback(
    (n: number, focus: number, prev: { start: number; end: number }) => {
      if (n === 0 || focus < 0) return { start: 0, end: 0 }
      let { start, end } = prev
      const span = WINDOW * 2 + 1
      if (end <= start || end > n || start < 0) {
        start = Math.max(0, focus - WINDOW)
        end = Math.min(n, start + span)
        if (end - start < span) start = Math.max(0, end - span)
      } else if (focus - start < WIN_MARGIN) {
        start = Math.max(0, focus - WINDOW)
        end = Math.min(n, start + span)
      } else if (end - 1 - focus < WIN_MARGIN) {
        end = Math.min(n, focus + WINDOW + 1)
        start = Math.max(0, end - span)
      } else {
        return prev
      }
      if (end - start < Math.min(span, n)) {
        start = Math.max(0, Math.min(start, n - Math.min(span, n)))
        end = Math.min(n, start + span)
      }
      return start === prev.start && end === prev.end ? prev : { start, end }
    },
    [],
  )

  const [winRange, setWinRange] = useState(() => {
    const cleaned = cleanLyricLines(lyrics)
    const n = cleaned.length
    const k0 = mapFocusIdx(cleaned, currentLyricIndex)
    return computeWinRange(n, k0 >= 0 ? k0 : 0, { start: 0, end: 0 })
  })
  useEffect(() => {
    setWinRange((prev) =>
      computeWinRange(cleanLines.length, focusIdx, prev),
    )
  }, [focusIdx, cleanLines.length, computeWinRange])

  const winStart = winRange.start
  const winEnd = winRange.end
  const windowLines = useMemo(
    () => cleanLines.slice(winStart, winEnd),
    [cleanLines, winStart, winEnd],
  )

  const [layoutReady, setLayoutReady] = useState(false)
  const viewportRef = useRef<HTMLDivElement>(null)
  const rowElsRef = useRef<Map<number, HTMLElement>>(new Map())
  const heightsRef = useRef<number[]>([])
  const itemsRef = useRef<WaveItem[]>([])
  const targetSRef = useRef(0)
  const viewHRef = useRef(0)
  const focusKRef = useRef(-1)
  const measuredRef = useRef(false)
  const rafRef = useRef(0)
  const lastTRef = useRef(0)
  const lyricsKeyRef = useRef('')
  const prevFocusRef = useRef(-1)
  const focusIdxRef = useRef(focusIdx)
  const winRangeRef = useRef(winRange)
  const cleanLinesRef = useRef(cleanLines)
  const pausedRef = useRef(paused)
  focusIdxRef.current = focusIdx
  winRangeRef.current = winRange
  cleanLinesRef.current = cleanLines
  pausedRef.current = paused

  const stopWave = useCallback(() => {
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = 0
    }
  }, [])

  const writeItemTransform = useCallback((it: WaveItem) => {
    if (!it.el) return
    const yAdjust = (it.h * (1 - it.scale)) / 2
    const drawY = it.pos + yAdjust
    const wy = Math.round(drawY * 100)
    const ws = Math.round(it.scale * 10000)
    if (it._wy === wy && it._ws === ws) return
    it._wy = wy
    it._ws = ws
    it.el.style.transform = `translate3d(0,${drawY.toFixed(2)}px,0) scale(${it.scale.toFixed(4)})`
  }, [])

  const snapAll = useCallback(() => {
    const items = itemsRef.current
    const s = targetSRef.current
    const viewH = viewHRef.current
    const focusK = focusKRef.current
    for (let k = 0; k < items.length; k++) {
      const it = items[k]
      it.pos = it.y - s
      it.v = 0
      it.scale = it.targetScale
      it.scaleV = 0
      if (it.idx === focusK) {
        it.pos = clampActivePos(it.pos, it.h, it.scale, viewH)
      }
      writeItemTransform(it)
    }
  }, [writeItemTransform])

  const rebuildWindowItems = useCallback(
    (lines: { text: string; origIdx: number }[], k0: number) => {
      const n = lines.length
      const heights = heightsRef.current
      if (heights.length !== n) {
        const next: number[] = Array.from({ length: n }, (_, i) =>
          heights[i] > 0 ? heights[i]! : EST_LINE_H,
        )
        heightsRef.current = next
      }
      const H = heightsRef.current
      const ys: number[] = Array.from({ length: n }, () => 0)
      let y = 0
      for (let i = 0; i < n; i++) {
        ys[i] = y
        y += H[i] || EST_LINE_H
      }

      const { start, end } = winRangeRef.current
      const prevByIdx = new Map(
        itemsRef.current.map((it) => [it.idx, it] as const),
      )
      const s = targetSRef.current
      const items: WaveItem[] = []
      for (let i = start; i < end; i++) {
        const el = rowElsRef.current.get(i) ?? null
        const old = prevByIdx.get(i)
        const h = H[i] || EST_LINE_H
        const absY = ys[i]
        if (old) {
          old.el = el
          old.y = absY
          old.h = h
          old.text = lines[i].text
          items.push(old)
        } else {
          items.push({
            idx: i,
            text: lines[i].text,
            el,
            y: absY,
            h,
            pos: absY - s,
            v: 0,
            scale: i === k0 ? SCALE_ACTIVE : SCALE_INACTIVE,
            scaleV: 0,
            targetScale: i === k0 ? SCALE_ACTIVE : SCALE_INACTIVE,
            delayUntil: 0,
            _wy: NaN,
            _ws: NaN,
          })
        }
      }
      itemsRef.current = items
    },
    [],
  )

  const measureLayout = useCallback((): boolean => {
    const vp = viewportRef.current
    const items = itemsRef.current
    if (!vp || items.length === 0) return false
    const outerH = vp.clientHeight
    const w = vp.clientWidth
    if (outerH < 24 || w < 40) return false
    void vp.offsetHeight
    const inner = vp.querySelector('.lyric-wave__inner') as HTMLElement | null
    const h = Math.max(24, inner?.clientHeight || outerH)

    const heights = heightsRef.current
    let measured = 0
    let sumH = 0
    for (let k = 0; k < items.length; k++) {
      const it = items[k]
      const el = rowElsRef.current.get(it.idx) ?? it.el
      it.el = el
      if (!el) continue
      const prev = el.style.transform
      el.style.transform = 'none'
      const hh = Math.max(el.offsetHeight || 0, el.scrollHeight || 0)
      el.style.transform = prev
      if (hh > 0) {
        it.h = hh
        heights[it.idx] = hh
        measured++
        sumH += hh
      }
    }
    if (measured > 0) {
      const avg = sumH / measured
      for (let i = 0; i < heights.length; i++) {
        if (!(heights[i] > 0)) heights[i] = avg
      }
    }

    let y = 0
    const ys: number[] = Array.from({ length: heights.length })
    for (let i = 0; i < heights.length; i++) {
      ys[i] = y
      y += heights[i] || EST_LINE_H
    }
    for (let k = 0; k < items.length; k++) {
      const it = items[k]
      it.y = ys[it.idx] ?? it.y
      it.h = heights[it.idx] || it.h
    }

    viewHRef.current = h
    measuredRef.current = true
    return true
  }, [])

  const startWave = useCallback(() => {
    if (pausedRef.current) return
    if (rafRef.current) return
    if (preferReducedMotion()) {
      snapAll()
      return
    }
    lastTRef.current = performance.now()
    const tick = (now: number) => {
      if (pausedRef.current) {
        rafRef.current = 0
        return
      }
      const dt = Math.min(0.032, (now - lastTRef.current) / 1000)
      lastTRef.current = now
      const items = itemsRef.current
      const s = targetSRef.current
      const viewH = viewHRef.current
      const focusK = focusKRef.current
      let moving = false
      const cullTop = -viewH
      const cullBot = viewH * 2

      for (let k = 0; k < items.length; k++) {
        const it = items[k]
        const ty = it.y - s
        const culled =
          it.idx !== focusK &&
          ((ty < cullTop && it.pos < cullTop) ||
            (ty > cullBot && it.pos > cullBot))

        if (culled) {
          it.pos = ty
          it.v = 0
          it.scale = it.targetScale
          it.scaleV = 0
          it._wy = NaN
          it._ws = NaN
          if (it.el) it.el.style.visibility = 'hidden'
          continue
        }
        if (it.el) it.el.style.visibility = ''

        if (now >= it.delayUntil) {
          const a = K * (ty - it.pos) - C * it.v
          it.v += a * dt
          it.pos += it.v * dt
          if (Math.abs(ty - it.pos) < 0.4 && Math.abs(it.v) < 3) {
            it.pos = ty
            it.v = 0
          } else {
            moving = true
          }
        } else {
          moving = true
        }

        const as = KS * (it.targetScale - it.scale) - CS * it.scaleV
        it.scaleV += as * dt
        it.scale += it.scaleV * dt
        if (
          Math.abs(it.targetScale - it.scale) < 0.002 &&
          Math.abs(it.scaleV) < 0.02
        ) {
          it.scale = it.targetScale
          it.scaleV = 0
        } else {
          moving = true
        }

        if (it.idx === focusK) {
          const clamped = clampActivePos(it.pos, it.h, it.scale, viewH)
          if (clamped !== it.pos) {
            it.pos = clamped
            it.v = 0
          }
        }

        writeItemTransform(it)
      }

      if (moving) rafRef.current = requestAnimationFrame(tick)
      else rafRef.current = 0
    }
    rafRef.current = requestAnimationFrame(tick)
  }, [snapAll, writeItemTransform])

  const focusItem = useCallback(
    (globalK: number, instant: boolean) => {
      const items = itemsRef.current
      if (items.length === 0) return
      if (!measuredRef.current && !measureLayout()) return

      // 先切 class 再测高，否则安全区按小字号算会裁切。
      for (let j = 0; j < items.length; j++) {
        const o = items[j]
        if (o.el) o.el.className = rowClass(o.idx, globalK)
      }
      void viewportRef.current?.offsetHeight

      const heights = heightsRef.current
      for (let j = 0; j < items.length; j++) {
        const o = items[j]
        if (!o.el) continue
        const prev = o.el.style.transform
        o.el.style.transform = 'none'
        const hh = Math.max(o.el.offsetHeight || 0, o.el.scrollHeight || 0)
        o.el.style.transform = prev
        if (hh > 0) {
          o.h = hh
          heights[o.idx] = hh
        }
      }
      let yAcc = 0
      const ys: number[] = Array.from({ length: heights.length })
      for (let i = 0; i < heights.length; i++) {
        ys[i] = yAcc
        yAcc += heights[i] || EST_LINE_H
      }
      for (let j = 0; j < items.length; j++) {
        const o = items[j]
        o.y = ys[o.idx] ?? o.y
        o.h = heights[o.idx] || o.h
      }

      const focusItemLocal = items.find((it) => it.idx === globalK)
      if (!focusItemLocal) {
        let y = 0
        for (let i = 0; i < globalK; i++) y += heights[i] || EST_LINE_H
        const hh = heights[globalK] || EST_LINE_H
        const viewH = viewHRef.current
        const raw = y - viewH * FOCAL + hh / 2
        targetSRef.current = clampFocusS(y, hh, viewH, raw)
        focusKRef.current = globalK
        prevFocusRef.current = globalK
        return
      }

      const viewH = viewHRef.current
      const rawDesired =
        focusItemLocal.y - viewH * FOCAL + focusItemLocal.h / 2
      const desiredS = clampFocusS(
        focusItemLocal.y,
        focusItemLocal.h,
        viewH,
        rawDesired,
      )
      const samePos =
        !instant &&
        focusKRef.current === globalK &&
        Math.abs(desiredS - targetSRef.current) < 1

      const now = performance.now()
      let scaleChanged = false
      for (let j = 0; j < items.length; j++) {
        const o = items[j]
        if (!samePos) {
          const d = o.idx - globalK
          o.delayUntil =
            d > 0 ? now + Math.min(d, WAVE_SPAN) * WAVE_DELAY : now
        }
        const ts = o.idx === globalK ? SCALE_ACTIVE : SCALE_INACTIVE
        if (ts !== o.targetScale) {
          o.targetScale = ts
          scaleChanged = true
        }
      }
      targetSRef.current = desiredS
      focusKRef.current = globalK
      prevFocusRef.current = globalK

      if (instant || preferReducedMotion()) {
        stopWave()
        snapAll()
        return
      }
      if (!samePos || scaleChanged) startWave()
    },
    [measureLayout, snapAll, startWave, stopWave],
  )

  const lyricsSig = useMemo(
    () => cleanLines.map((l) => l.text).join('\n'),
    [cleanLines],
  )

  useLayoutEffect(() => {
    const lines = cleanLinesRef.current
    if (lines.length === 0 || winEnd <= winStart) {
      itemsRef.current = []
      heightsRef.current = []
      measuredRef.current = false
      focusKRef.current = -1
      prevFocusRef.current = -1
      lyricsKeyRef.current = ''
      setLayoutReady(false)
      stopWave()
      return
    }

    const sigChanged = lyricsKeyRef.current !== lyricsSig
    if (sigChanged) {
      lyricsKeyRef.current = lyricsSig
      heightsRef.current = lines.map(() => EST_LINE_H)
      setLayoutReady(false)
      prevFocusRef.current = -1
      itemsRef.current = []
    }

    const k0 = focusIdxRef.current >= 0 ? focusIdxRef.current : 0
    rebuildWindowItems(lines, k0)

    let cancelled = false
    let attempt = 0
    let raf1 = 0
    let raf2 = 0
    let retryTimer = 0
    const maxAttempts = 12

    const settle = () => {
      if (cancelled) return
      rebuildWindowItems(lines, k0)
      if (!measureLayout()) {
        if (attempt++ >= maxAttempts) {
          snapAll()
          setLayoutReady(true)
          return
        }
        retryTimer = window.setTimeout(() => {
          raf1 = requestAnimationFrame(settle)
        }, attempt < 3 ? 0 : 32)
        return
      }
      rebuildWindowItems(lines, k0)
      measureLayout()
      if (sigChanged || prevFocusRef.current < 0) {
        focusItem(k0, true)
      }
      setLayoutReady(true)
    }

    raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(settle)
    })
    return () => {
      cancelled = true
      cancelAnimationFrame(raf1)
      cancelAnimationFrame(raf2)
      if (retryTimer) window.clearTimeout(retryTimer)
    }
  }, [
    lyricsSig,
    winStart,
    winEnd,
    rebuildWindowItems,
    measureLayout,
    focusItem,
    stopWave,
    snapAll,
  ])

  useEffect(() => {
    if (cleanLines.length === 0 || focusIdx < 0) return
    if (!measuredRef.current || !layoutReady) return
    const prev = prevFocusRef.current
    if (prev === focusIdx) return
    const seek = prev >= 0 && Math.abs(focusIdx - prev) > SEEK_JUMP
    if (prev < 0) {
      if (focusKRef.current !== focusIdx) focusItem(focusIdx, true)
      return
    }
    focusItem(focusIdx, seek)
  }, [focusIdx, cleanLines.length, focusItem, layoutReady])

  useEffect(() => {
    const vp = viewportRef.current
    if (!vp) return
    const ro = new ResizeObserver(() => {
      if (itemsRef.current.length === 0) return
      measuredRef.current = false
      if (measureLayout()) {
        const k = focusKRef.current >= 0 ? focusKRef.current : 0
        focusItem(k, true)
        // 量高成功后必须亮起，否则资料库歌词会一直 opacity:0。
        setLayoutReady(true)
      }
    })
    ro.observe(vp)
    return () => ro.disconnect()
  }, [measureLayout, focusItem])

  useEffect(() => () => stopWave(), [stopWave])

  useEffect(() => {
    if (paused) {
      stopWave()
      return
    }
    if (itemsRef.current.length === 0) return
    let cancelled = false
    let raf1 = 0
    let raf2 = 0
    let retryTimer = 0
    let attempts = 0
    const tryResume = () => {
      if (cancelled || pausedRef.current) return
      measuredRef.current = false
      if (measureLayout()) {
        const k = focusKRef.current >= 0 ? focusKRef.current : 0
        focusItem(k, true)
        setLayoutReady(true)
        return
      }
      if (attempts++ < 8) {
        retryTimer = window.setTimeout(() => {
          raf1 = requestAnimationFrame(tryResume)
        }, attempts < 3 ? 16 : 40)
      } else {
        setLayoutReady(true)
      }
    }
    raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(tryResume)
    })
    return () => {
      cancelled = true
      cancelAnimationFrame(raf1)
      cancelAnimationFrame(raf2)
      if (retryTimer) window.clearTimeout(retryTimer)
    }
  }, [paused, stopWave, measureLayout, focusItem])

  const setRowRef = useCallback(
    (globalIdx: number, el: HTMLParagraphElement | null) => {
      if (el) rowElsRef.current.set(globalIdx, el)
      else rowElsRef.current.delete(globalIdx)
      const it = itemsRef.current.find((x) => x.idx === globalIdx)
      if (it) it.el = el
    },
    [],
  )

  if (cleanLines.length === 0) return null

  const focusForClass = focusIdx >= 0 ? focusIdx : 0

  return (
    <div
      ref={viewportRef}
      className={`lyric-wave lyric-wave--${variant}${className ? ` ${className}` : ''}`}
      style={
        {
          '--lyric-wave-color': musicColor,
          opacity: layoutReady ? 1 : 0,
          transition: layoutReady ? 'opacity 0.2s ease' : 'none',
        } as CSSProperties
      }
      aria-hidden
      data-total={cleanLines.length}
      data-focus={focusForClass}
    >
      <div className="lyric-wave__inner">
        {windowLines.map((line, j) => {
          const i = winStart + j
          return (
            <p
              key={`${line.origIdx}-${i}`}
              ref={(el) => setRowRef(i, el)}
              className={rowClass(i, focusForClass)}
              data-index={i}
            >
              {line.text}
            </p>
          )
        })}
      </div>
    </div>
  )
})

export default LyricWaveScroll
