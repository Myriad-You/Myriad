import type { DependencyList, RefCallback } from 'react'
import { useCallback, useLayoutEffect, useRef, useState } from 'react'
import { scheduleFitText } from './fitTextScheduler'
import { isReducedAnimation, useAnimationLevel } from './useAnimationLevel'

/** FitText：single / wrap / marquee 取最小 badness；wrap 须有干净断点。 */
export type FitTextMode = 'single' | 'wrap' | 'marquee'

export interface FitTextOptions {
  /** 缺省取元素 CSS 计算字号。 */
  max?: number
  /** 缺省 max 的一半，不低于 10px。 */
  min?: number
  maxLines?: number
  boxHeight?: number
  /** false 时超长退省略号。 */
  marquee?: boolean
  /** 引擎已观察内容/宽度/字体，一般不用传。 */
  deps?: DependencyList
  enabled?: boolean
}

export interface FitTextResult {
  ref: RefCallback<HTMLElement>
  fontSize: number
  lineHeight: number
  mode: FitTextMode
  /** 到下限仍放不下且无法滚动时需要省略号。 */
  clamped: boolean
  marqueeDistance: number
  marqueeDuration: number
}

interface FitState {
  fontSize: number
  lineHeight: number
  mode: FitTextMode
  clamped: boolean
  marqueeDistance: number
  marqueeDuration: number
}

const TOL = 0.5

/** 低于此宽度抖动不重测，避免过渡中间帧强制重排。 */
const REFIT_MIN_DELTA = 4

/** 自动推导的 min 不低于 10px。 */
const ABS_MIN_FONT = 10

const WRAP_LINE_PENALTY = 10

/** 须明显优于缩到下限才启用滚动。 */
const MARQUEE_PENALTY = 70

/** 截断是最后手段。 */
const CLAMP_PENALTY = 1000

/** 滚动已解决容纳，不必再缩很小。 */
const MARQUEE_SIZE_RATIO = 0.9

const MARQUEE_SPEED = 28

const MARQUEE_LEAD_PX = 12

/** 只约束生长占宽；溢出判定仍用全宽。 */
const RESTRAINT = 0.92

/** 无干净断点则禁止换行，避免连续 CJK 词中断开。 */
const BREAK_RE = /[-\s/\u00AD\u200B\u3001\u3002\uFF0C\uFF01\uFF1F\uFF1A\uFF1B\u30FB]/

function lineHeightFor(fontSize: number): number {
  return Math.min(1.42, Math.max(1.08, 1.52 - 0.011 * fontSize))
}

function sizeBadness(s: number, min: number, ideal: number): number {
  if (ideal <= min) return 0
  const r = (ideal - s) / (ideal - min)
  return r * r * r * 100
}

/** 每轮一次强制同步重排；6 轮把 [min,max] 收到约 (max-min)/64。 */
const BISECT_STEPS = 6

/**
 * 预测宽度与阈值相差不到这个量时改为实测。取算法自身容差 TOL：
 * 同一轮内模型误差在 LayoutUnit 量级（约 0.06px），低于比较本身的精度。
 */
const PREDICT_GUARD_PX = TOL

/** 先试上限，放得下直接用。 */
function largestFitting(
  lo: number,
  hi: number,
  apply: (s: number) => void,
  fits: () => boolean,
): number {
  apply(hi)
  if (fits()) return hi
  let best = lo
  let l = lo
  let h = hi
  for (let i = 0; i < BISECT_STEPS; i++) {
    const mid = (l + h) / 2
    apply(mid)
    if (fits()) {
      best = mid
      l = mid
    } else {
      h = mid
    }
  }
  apply(best)
  return best
}

interface WidthSample {
  size: number
  width: number
}

/**
 * 单行宽度对字号是仿射的：字形按字号等比缩放，图标与 px 间距不变。
 * 二分各轮按模型判定，只有贴近阈值时才改字号实测，走出的路径与结果
 * 和逐轮实测相同。实测贵在每个新字号都要实例化字体（CJK 还要走回退），
 * 所以能用已渲染字号的样本（proportional）就不去碰新字号。
 */
function largestFittingWidth(
  lo: number,
  hi: number,
  apply: (s: number) => void,
  width: () => number,
  limit: number,
  proportional: WidthSample | null,
): { size: number, predict: (s: number) => number } {
  let predict: (s: number) => number
  if (proportional) {
    const { size, width: w } = proportional
    predict = s => (w * s) / size
  } else {
    apply(hi)
    const wHi = width()
    if (wHi <= limit || hi <= lo) {
      const size = wHi <= limit ? hi : lo
      if (size !== hi) apply(size)
      return { size, predict: () => wHi }
    }
    apply(lo)
    const wLo = width()
    predict = s => wLo + ((wHi - wLo) * (s - lo)) / (hi - lo)
  }
  const fits = (s: number) => {
    const predicted = predict(s)
    if (Math.abs(predicted - limit) > PREDICT_GUARD_PX) return predicted <= limit
    apply(s)
    return width() <= limit
  }
  if (fits(hi)) {
    apply(hi)
    return { size: hi, predict }
  }
  let best = lo
  let l = lo
  let h = hi
  for (let i = 0; i < BISECT_STEPS; i++) {
    const mid = (l + h) / 2
    if (fits(mid)) {
      best = mid
      l = mid
    } else {
      h = mid
    }
  }
  apply(best)
  return { size: best, predict }
}

/** 纯文本、字距与词距为默认值时，宽度与字号严格成正比。 */
function isProportionalText(el: HTMLElement, style: CSSStyleDeclaration): boolean {
  return style.letterSpacing === 'normal'
    && style.wordSpacing === '0px'
    && el.querySelector(':not([data-fittext-track])') === null
}

export function useFitText(options: FitTextOptions): FitTextResult {
  const {
    max,
    min,
    maxLines = 1,
    boxHeight,
    marquee = true,
    deps = [],
    enabled,
  } = options

  const anim = useAnimationLevel()

  // 低端只挂载后补测一次，不持续挂 ResizeObserver。
  const reducedPerf = isReducedAnimation(anim)
  const active = enabled ?? !reducedPerf
  const marqueeAllowed = marquee !== false && active && !!anim.loop

  const [node, setNode] = useState<HTMLElement | null>(null)
  const initialSize = max ?? 16
  const [state, setState] = useState<FitState>({
    fontSize: initialSize,
    lineHeight: lineHeightFor(initialSize),
    mode: 'single',
    clamped: false,
    marqueeDistance: 0,
    marqueeDuration: 0,
  })

  const ref = useCallback<RefCallback<HTMLElement>>((el) => setNode(el), [])

  // 未传 max 时首测捕获 CSS 计算字号。
  const idealRef = useRef<number | null>(null)

  // RO 过滤非宽度变化，避免字号改高度回环。
  const widthsRef = useRef({ el: -1, parent: -1 })
  const fittingRef = useRef(false)

  const paramsRef = useRef({ max, min, maxLines, boxHeight, marqueeAllowed })
  paramsRef.current = { max, min, maxLines, boxHeight, marqueeAllowed }

  const runFit = useCallback((el: HTMLElement) => {
    if (fittingRef.current) return
    fittingRef.current = true
    try {
    const { max, min, maxLines, boxHeight, marqueeAllowed } = paramsRef.current

    // 宽度为 0 时不测，等观察器拿到真实宽度。
    if (el.clientWidth <= 0) return

    let ideal = max
    if (ideal == null) {
      if (idealRef.current == null) {
        const prev = el.style.fontSize
        el.style.fontSize = ''
        idealRef.current
          = Number.parseFloat(getComputedStyle(el).fontSize) || 16
        el.style.fontSize = prev
      }
      ideal = idealRef.current
    }
    const floor = Math.min(
      min ?? Math.max(ABS_MIN_FONT, Math.round(ideal * 0.5)),
      ideal,
    )

    // 测量期间归零轨道，避免动画位移干扰读数。
    const track = el.querySelector<HTMLElement>('[data-fittext-track]')
    const savedTrackCss = track?.style.cssText
    if (track) {
      track.style.animation = 'none'
      track.style.transform = 'none'
    }

    const text = (el.textContent ?? '').trim()
    const hasCleanBreak = BREAK_RE.test(text)

    // 用 Range 测内容宽：scrollWidth 会被容器钳位，短文本会假溢出缩到最小。
    const range = document.createRange()
    const contentWidth = () => {
      range.selectNodeContents(el)
      return range.getBoundingClientRect().width
    }

    // 容器宽整轮只读一次：块级宽度不随字号变，重复读只是多一次强制重排。
    const measuredBoxWidth = el.getBoundingClientRect().width

    // 生长 ≤ RESTRAINT；溢出判定用全宽。
    const widthOkGrow = () =>
      contentWidth() <= measuredBoxWidth * RESTRAINT + TOL
    const widthOkHard = () => contentWidth() <= measuredBoxWidth + TOL
    const applySize = (s: number) => {
      el.style.fontSize = `${s}px`
      el.style.lineHeight = String(lineHeightFor(s))
    }

    el.style.whiteSpace = 'nowrap'
    const hardLimit = measuredBoxWidth + TOL
    const rendered = getComputedStyle(el)
    const renderedSize = Number.parseFloat(rendered.fontSize)
    let sample: WidthSample | null = null
    if (renderedSize > 0 && isProportionalText(el, rendered)) {
      const renderedWidth = contentWidth()
      if (renderedWidth > 0) sample = { size: renderedSize, width: renderedWidth }
    }
    const single = largestFittingWidth(
      floor,
      ideal,
      applySize,
      contentWidth,
      measuredBoxWidth * RESTRAINT + TOL,
      sample,
    )
    const sSingle = single.size
    const predictedSingle = single.predict(sSingle)
    const singleFits = Math.abs(predictedSingle - hardLimit) > PREDICT_GUARD_PX
      ? predictedSingle <= hardLimit
      : widthOkHard()
    let best = {
      mode: 'single' as FitTextMode,
      size: sSingle,
      badness:
        sizeBadness(sSingle, floor, ideal) + (singleFits ? 0 : CLAMP_PENALTY),
      clamped: !singleFits,
      distance: 0,
    }

    if (maxLines > 1 && hasCleanBreak) {
      el.style.whiteSpace = 'normal'
      const heightOk = () => {
        const s = Number.parseFloat(el.style.fontSize)
        const limit
          = boxHeight && boxHeight > 0
            ? boxHeight
            : maxLines * s * lineHeightFor(s) + 2
        return el.scrollHeight <= limit + TOL
      }
      const bothOk = () => widthOkGrow() && heightOk()
      const sWrap = largestFitting(floor, ideal, applySize, bothOk)
      if (widthOkHard() && heightOk()) {
        const lines = Math.max(
          2,
          Math.round(el.scrollHeight / (sWrap * lineHeightFor(sWrap))),
        )
        const badness
          = sizeBadness(sWrap, floor, ideal) + WRAP_LINE_PENALTY * (lines - 1)
        if (badness < best.badness) {
          best = { mode: 'wrap', size: sWrap, badness, clamped: false, distance: 0 }
        }
      }
    }

    if (!singleFits && marqueeAllowed) {
      const sMarquee = Math.max(floor, ideal * MARQUEE_SIZE_RATIO)
      const badness = sizeBadness(sMarquee, floor, ideal) + MARQUEE_PENALTY
      if (badness < best.badness) {
        el.style.whiteSpace = 'nowrap'
        applySize(sMarquee)
        const distance
          = Math.max(0, contentWidth() - measuredBoxWidth) + MARQUEE_LEAD_PX
        best = { mode: 'marquee', size: sMarquee, badness, clamped: false, distance }
      }
    }

    if (track && savedTrackCss !== undefined) {
      track.style.cssText = savedTrackCss
    }

    // 立刻写回最终布局，避免测量残留闪到下一帧 React 渲染。
    el.style.whiteSpace = best.mode === 'wrap' ? 'normal' : 'nowrap'
    applySize(best.size)

    const next: FitState = {
      fontSize: best.size,
      lineHeight: lineHeightFor(best.size),
      mode: best.mode,
      clamped: best.clamped,
      marqueeDistance: best.mode === 'marquee' ? best.distance : 0,
      marqueeDuration:
        best.mode === 'marquee'
          ? Math.min(30, Math.max(6, (best.distance / MARQUEE_SPEED) * 2 + 3))
          : 0,
    }
    setState((prev) =>
      prev.fontSize === next.fontSize
      && prev.lineHeight === next.lineHeight
      && prev.mode === next.mode
      && prev.clamped === next.clamped
      && prev.marqueeDistance === next.marqueeDistance
      && prev.marqueeDuration === next.marqueeDuration
        ? prev
        : next,
    )

    widthsRef.current = {
      el: el.clientWidth,
      parent: el.parentElement?.clientWidth ?? -1,
    }
    } finally {
      fittingRef.current = false
    }
  }, [])

  useLayoutEffect(() => {
    if (!node) return

    const fit = () => {
      if (node.isConnected) runFit(node)
    }
    let cancel = scheduleFitText(node, fit)
    const schedule = () => {
      cancel()
      cancel = scheduleFitText(node, fit)
    }

    if (!active) {
      return () => cancel()
    }

    // 阈值用 REFIT_MIN_DELTA 而非 1px，否则过渡亚像素抖动会连串强制重排。
    const ro = new ResizeObserver(() => {
      if (fittingRef.current) return
      const prev = widthsRef.current
      const elW = node.clientWidth
      const parentW = node.parentElement?.clientWidth ?? -1
      if (
        Math.abs(elW - prev.el) < REFIT_MIN_DELTA &&
        Math.abs(parentW - prev.parent) < REFIT_MIN_DELTA
      ) {
        return
      }
      schedule()
    })
    ro.observe(node)
    if (node.parentElement) ro.observe(node.parentElement)

    const mo = new MutationObserver(() => {
      if (fittingRef.current) return
      schedule()
    })
    mo.observe(node, { childList: true, characterData: true, subtree: true })

    let cancelled = false
    // webfont 就位后字宽变化，补测一次。
    document.fonts?.ready.then(() => {
      if (!cancelled) schedule()
    })

    const io =
      typeof IntersectionObserver === 'undefined'
        ? null
        : new IntersectionObserver((entries) => {
            node.toggleAttribute(
              'data-offscreen',
              !entries.some((entry) => entry.isIntersecting),
            )
          })
    io?.observe(node)

    return () => {
      cancelled = true
      ro.disconnect()
      mo.disconnect()
      io?.disconnect()
      cancel()
    }
  }, [node, active, max, min, maxLines, boxHeight, marqueeAllowed, runFit, ...deps])

  return {
    ref,
    fontSize: state.fontSize,
    lineHeight: state.lineHeight,
    mode: state.mode,
    clamped: state.clamped,
    marqueeDistance: state.marqueeDistance,
    marqueeDuration: state.marqueeDuration,
  }
}
