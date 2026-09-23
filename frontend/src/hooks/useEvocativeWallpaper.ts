import { useEffect, useRef } from 'react'
import { effectiveWallpaperBlur } from '../utils/wallpaperState'
import { batchWrite, isPageVisible, onVisibility } from './animation/core'
import { createFrameClock } from './animation/frameClock'

// 视差阻尼：大屏上鼠标随手一动整张壁纸就跟着晃，跟随要慢于模糊。
const PARALLAX_SMOOTH = 0.03
const PARALLAX_SMOOTH_RETURN = 0.02
const BLUR_SMOOTH = 0.08
const BLUR_SMOOTH_RETURN = 0.04
/** 指针离开内容后延迟清晰，避免划过卡片间缝隙时忽清忽糊。 */
const UNBLUR_DELAY_MS = 180
const MAX_DELTA = 100
const THRESHOLD = 0.05

const PARALLAX_SCALE = 1.02
const PARALLAX_MAX_OFFSET = 8
const GYRO_SENS = 0.5
/** 进资料库画布 / 离场恢复：soft-lock 与 soft-restore 的 scale·位移过渡 */
const EFFECT_EDGE_MS = 960
const EFFECT_EDGE_EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'
const EFFECT_EDGE_TRANSITION = `transform ${EFFECT_EDGE_MS}ms ${EFFECT_EDGE_EASE}`
const IDENTITY_TF = 'scale(1) translate3d(0,0,0)'

function isLibraryCanvasHoldingWallpaper(): boolean {
  return (
    typeof document !== 'undefined' &&
    document.documentElement.dataset.libraryCanvas === 'active'
  )
}

/**
 * Soft-lock wallpaper to identity (all clients).
 * Cache the current computed transform as the from-value, then transition to
 * identity so entering library canvas (esp. from other routes) doesn't hard-cut
 * parallax scale/offset. Leaves transform at IDENTITY_TF so removing
 * data-library-canvas later won't snap back to a stale parallax matrix.
 *
 * Idempotent: LibraryGrid layout + evocative cleanup/effect body may all call
 * this when canvas activates; later calls must not restart the in-flight fade.
 */
function softLockWallpaperTransform(el: HTMLElement): void {
  // Already locked (or mid soft-lock after to-value was written).
  if (el.style.transform === IDENTITY_TF) return

  let computed = 'none'
  try {
    computed = getComputedStyle(el).transform
  } catch {
    // getComputedStyle can throw in detached documents
  }
  const from =
    !computed || computed === 'none' ? IDENTITY_TF : computed

  // Cache from-frame with transition disabled, then ease to identity.
  // Important beats residual stylesheet transition on #wallpaper (opacity rule).
  el.style.setProperty('transition', 'none', 'important')
  el.style.transform = from
  // Force style flush so the browser registers the from value before to-value.
  void el.offsetWidth
  el.style.setProperty('transition', EFFECT_EDGE_TRANSITION, 'important')
  el.style.transform = IDENTITY_TF
}

/** True when the pointer actually left the viewport, not a chrome hit-test drop. */
export function isWallpaperMouseLeaveFromViewport(
  relatedTarget: EventTarget | null,
): boolean {
  if (relatedTarget == null) return true
  if (typeof document === 'undefined' || typeof Node === 'undefined') return true
  if (!(relatedTarget instanceof Node)) return true
  return !document.contains(relatedTarget)
}

/**
 * Capture the live parallax matrix and ease #wallpaper to identity.
 * Called from LibraryGrid's layout effect so the from-frame is cached before
 * paint when navigating into /library canvas from another route.
 */
export function softLockWallpaperForLibraryCanvas(): void {
  if (typeof document === 'undefined') return
  const el = document.getElementById('wallpaper')
  if (el) softLockWallpaperTransform(el)
}

/** Clear inline transform/transition, including soft-lock's important transition. */
function clearWallpaperTransformStyles(el: HTMLElement): void {
  el.style.removeProperty('transition')
  el.style.transform = ''
  el.style.transformOrigin = ''
}

const STATIC_TF_PREFIX = `scale(${PARALLAX_SCALE}) translate3d(`
const STATIC_TF_SUFFIX = ',0)'
const IDLE_TF = `scale(${PARALLAX_SCALE}) translate3d(0,0,0)`

const UNBLUR_ZONE = 0.4

const CONTENT_TAGS = new Set([
  'A', 'BUTTON', 'INPUT', 'SELECT', 'TEXTAREA', 'LABEL',
  'IMG', 'SVG', 'CANVAS', 'VIDEO', 'AUDIO', 'IFRAME',
])

function hasOwnText(el: Element): boolean {
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE && node.textContent?.trim()) {
      return true
    }
  }
  return false
}

/**
 * 指针是否落在壁纸空白处：自身不是可交互/文本/媒体元素，且到 body 为止
 * 没有任何祖先带底色、背景图或毛玻璃（卡片、岛、弹层都会命中其一）。
 */
export function isPointerOverWallpaperBlank(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return true
  if (target === document.body || target === document.documentElement) {
    return true
  }
  if (CONTENT_TAGS.has(target.tagName.toUpperCase()) || hasOwnText(target)) {
    return false
  }
  for (
    let el: Element | null = target;
    el && el !== document.body && el !== document.documentElement;
    el = el.parentElement
  ) {
    const tag = el.tagName.toUpperCase()
    if (tag === 'svg'.toUpperCase() || CONTENT_TAGS.has(tag)) return false
    const cs = getComputedStyle(el)
    if (
      cs.backgroundImage !== 'none' ||
      (cs.backdropFilter && cs.backdropFilter !== 'none') ||
      !isTransparentColor(cs.backgroundColor)
    ) {
      return false
    }
  }
  return true
}

function isTransparentColor(color: string): boolean {
  if (color === 'transparent') return true
  const m = /rgba?\(([^)]+)\)/.exec(color)
  if (!m) return false
  const parts = m[1].split(/[\s,/]+/).filter(Boolean)
  return parts.length === 4 && Number.parseFloat(parts[3]) === 0
}
const BLUR_PREFIX = 'blur('
const BLUR_SUFFIX = 'px)'

const MAX_RIPPLES = 3
const RIPPLE_DURATION = 2000
const RIPPLE_SPEED = 400
const RIPPLE_WAVELENGTH = 80
const RIPPLE_AMPLITUDE = 15

const SIN_TABLE_SIZE = 1024
const SIN_TABLE = new Float32Array(SIN_TABLE_SIZE)
for (let i = 0; i < SIN_TABLE_SIZE; i++) {
  SIN_TABLE[i] = Math.sin((i / SIN_TABLE_SIZE) * Math.PI * 2)
}

const EXP_TABLE_SIZE = 256
const EXP_TABLE_MAX = 8
const EXP_TABLE = new Float32Array(EXP_TABLE_SIZE)
for (let i = 0; i < EXP_TABLE_SIZE; i++) {
  EXP_TABLE[i] = Math.exp(-(i / EXP_TABLE_SIZE) * EXP_TABLE_MAX)
}

function fastSin(x: number): number {
  const idx =
    (((x % (Math.PI * 2)) / (Math.PI * 2)) * SIN_TABLE_SIZE + SIN_TABLE_SIZE) %
    SIN_TABLE_SIZE
  return SIN_TABLE[idx | 0]
}

function fastExp(x: number): number {
  if (x >= 0) return 1
  const absX = -x
  if (absX >= EXP_TABLE_MAX) return 0
  return EXP_TABLE[((absX / EXP_TABLE_MAX) * EXP_TABLE_SIZE) | 0]
}

export interface ParallaxOptions {
  enabled?: boolean
  enableGyroscope?: boolean
  enableMouse?: boolean
  maxOffset?: number
  scale?: number
}

export interface DynamicBlurOptions {
  enabled?: boolean
  baseBlur?: number
  unblurZone?: number
  blurZone?: number
}

export interface RippleOptions {
  enabled?: boolean
}

export interface EvocativeOptions {
  parallax?: ParallaxOptions
  dynamicBlur?: DynamicBlurOptions
  ripple?: RippleOptions
  fps?: number
  rippleQuality?: number
}

interface EvocativeState {
  active: boolean
  pageVisible: boolean
  el: HTMLElement | null
  raf: number | null
  returning: boolean

  parallaxTx: number
  parallaxTy: number
  parallaxCx: number
  parallaxCy: number
  parallaxLastRx: number
  parallaxLastRy: number
  parallaxIdle: boolean
  parallaxOffsetMult: number
  parallaxGyroMult: number
  gyroEnabled: boolean
  permissionRequested: boolean
  reqHandler: (() => void) | null

  blurTargetBlur: number
  blurCurrentBlur: number
  blurBaseBlur: number
  blurLastRendered: number
  blurIdle: boolean

  rippleCanvas: HTMLCanvasElement | null
  rippleCtx: CanvasRenderingContext2D | null
  sourceImageData: ImageData | null
  destImageData: ImageData | null
  activeRipples: Array<{ x: number; y: number; startTime: number }>
  rippleRaf: number | null
  rippleFadeoutTimer: ReturnType<typeof setTimeout> | null
  rippleIsFadingOut: boolean
}

function buildTransform(cx: number, cy: number): string {
  const rx = Math.round(cx * 10) / 10
  const ry = Math.round(cy * 10) / 10
  return `${STATIC_TF_PREFIX}${rx}px,${ry}px${STATIC_TF_SUFFIX}`
}

function buildCanvasTransform(
  wallpaperEl: HTMLElement,
  parallaxEnabled: boolean,
): string {
  if (!parallaxEnabled) {
    return 'none'
  }
  const transform = wallpaperEl.style.transform
  if (!transform) return IDLE_TF
  const match = transform.match(/translate3d\([^)]+\)/)
  return match ? `scale(${PARALLAX_SCALE}) ${match[0]}` : IDLE_TF
}

/**
 * Ripple bitmap must match the visible #bg-container crop (lvh / fixed layer),
 * not window.inner* — those can disagree after the full-viewport wallpaper work.
 * Falls back to the window when the container is missing (tests / early mount).
 */
function getRippleViewportSize(): { width: number; height: number } {
  const container = document.getElementById('bg-container')
  if (container) {
    const width = container.clientWidth
    const height = container.clientHeight
    if (width > 0 && height > 0) return { width, height }
  }
  return { width: window.innerWidth, height: window.innerHeight }
}

/** Container client rect for mapping pointer → canvas-local coords. */
function getRippleViewportRect(): DOMRect {
  const container = document.getElementById('bg-container')
  if (container) {
    const rect = container.getBoundingClientRect()
    if (rect.width > 0 && rect.height > 0) return rect
  }
  return new DOMRect(0, 0, window.innerWidth, window.innerHeight)
}

function createRippleCanvas(
  wallpaperEl: HTMLElement,
  parallaxEnabled: boolean,
): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  canvas.id = 'wallpaper-ripple-canvas'
  // Bitmap = visible container crop (canvas DOM stays inset:0; #wallpaper is oversized).
  // Sized lazily by startRipple; an idle canvas holds no backing store.
  canvas.width = 0
  canvas.height = 0

  const computedStyle = window.getComputedStyle(wallpaperEl)
  const transformOrigin = parallaxEnabled
    ? computedStyle.transformOrigin || 'center'
    : 'center'
  const canvasTransform = buildCanvasTransform(wallpaperEl, parallaxEnabled)

  // z-index:1 — 夹在 #wallpaper 与 #bg-gradient(z-2) 之间，保证涟漪可见且不挡底部遮罩
  // DOM 贴满 #bg-container（inset:0 / 100%），不外扩；视差 transform 与壁纸镜像
  canvas.style.cssText = `
    position: absolute;
    inset: 0;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    z-index: 1;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s ease-out;
    transform: ${canvasTransform};
    transform-origin: ${transformOrigin};
    will-change: transform, opacity;
    image-rendering: auto;
  `

  const bgContainer = document.getElementById('bg-container')
  const wallpaper = document.getElementById('wallpaper')
  const bgGradient = document.getElementById('bg-gradient')

  if (bgContainer && wallpaper && bgGradient) {
    bgContainer.insertBefore(canvas, bgGradient)
  } else if (bgContainer && wallpaper) {
    wallpaper.insertAdjacentElement('afterend', canvas)
  } else {
    canvas.style.position = 'fixed'
    canvas.style.zIndex = '-1'
    document.body.appendChild(canvas)
  }

  return canvas
}

/**
 * Capture the wallpaper as the viewport-visible crop for the ripple bitmap.
 *
 * #wallpaper is oversized (inset:-4%) for CSS blur edges; background-size:cover
 * is relative to that larger box. Cover-fit to the real wallpaper size, then
 * draw with wallpaper offset relative to #bg-container so the canvas matches
 * what the user sees (not a cover-fit of the container alone).
 */
async function captureWallpaperToCanvas(
  wallpaperEl: HTMLElement,
  canvas: HTMLCanvasElement,
  ctx: CanvasRenderingContext2D,
  currentBlur: number,
  rippleScale: number,
): Promise<ImageData | null> {
  const computedStyle = window.getComputedStyle(wallpaperEl)
  const bgImage = computedStyle.backgroundImage

  if (!bgImage || bgImage === 'none') return null

  const urlMatch = bgImage.match(/url\(['"]?([^'"]+)['"]?\)/)
  if (!urlMatch) return null

  return new Promise((resolve) => {
    const img = new Image()
    img.crossOrigin = 'anonymous'

    img.onload = () => {
      const canvasW = canvas.width
      const canvasH = canvas.height

      // Layout box of #wallpaper (includes inset:-4% oversize). Prefer offset*
      // over getBoundingClientRect so parallax scale(1.02) does not inflate
      // cover-fit — background-size:cover is relative to the layout box, and
      // the ripple canvas mirrors the same transform in CSS.
      const container = document.getElementById('bg-container')
      let wpCssW = wallpaperEl.offsetWidth
      let wpCssH = wallpaperEl.offsetHeight
      let offsetCssX: number
      let offsetCssY: number

      if (container && wallpaperEl.offsetParent === container) {
        offsetCssX = wallpaperEl.offsetLeft
        offsetCssY = wallpaperEl.offsetTop
      } else {
        const wallpaperRect = wallpaperEl.getBoundingClientRect()
        const containerRect = getRippleViewportRect()
        if (wpCssW <= 0) wpCssW = wallpaperRect.width
        if (wpCssH <= 0) wpCssH = wallpaperRect.height
        offsetCssX = wallpaperRect.left - containerRect.left
        offsetCssY = wallpaperRect.top - containerRect.top
      }

      const wpW = Math.max(1, wpCssW * rippleScale)
      const wpH = Math.max(1, wpCssH * rippleScale)

      // background-size: cover relative to the real #wallpaper element
      const imgRatio = img.width / img.height
      const wpRatio = wpW / wpH

      let drawW: number, drawH: number
      if (imgRatio > wpRatio) {
        drawH = wpH
        drawW = drawH * imgRatio
      } else {
        drawW = wpW
        drawH = drawW / imgRatio
      }

      const imgOnWpX = (wpW - drawW) / 2
      const imgOnWpY = (wpH - drawH) / 2

      const offsetX = offsetCssX * rippleScale
      const offsetY = offsetCssY * rippleScale

      const drawX = offsetX + imgOnWpX
      const drawY = offsetY + imgOnWpY

      ctx.clearRect(0, 0, canvasW, canvasH)
      const scaledBlur = currentBlur * rippleScale
      ctx.filter = scaledBlur > 0 ? `blur(${scaledBlur}px)` : 'none'
      ctx.drawImage(img, drawX, drawY, drawW, drawH)
      ctx.filter = 'none'

      try {
        resolve(ctx.getImageData(0, 0, canvasW, canvasH))
      } catch {
        resolve(null)
      }
    }

    img.onerror = () => resolve(null)
    img.src = urlMatch[1]
  })
}

function applyRippleDistortion(
  ctx: CanvasRenderingContext2D,
  sourceData: ImageData,
  destData: ImageData | null,
  width: number,
  height: number,
  ripples: Array<{ x: number; y: number; startTime: number }>,
  now: number,
  rippleScale: number,
): { hasActive: boolean; destData: ImageData } {
  const src32 = new Uint32Array(sourceData.data.buffer)

  const destImageData =
    destData?.width === width && destData.height === height
      ? destData
      : ctx.createImageData(width, height)
  const dest32 = new Uint32Array(destImageData.data.buffer)

  dest32.set(src32)

  let hasActiveRipple = false
  const activeCount = ripples.length
  if (activeCount === 0) {
    ctx.putImageData(destImageData, 0, 0)
    return { hasActive: false, destData: destImageData }
  }

  const durationSec = RIPPLE_DURATION / 1000
  const waveWidth = RIPPLE_WAVELENGTH * 2
  const wavelengthSqScaled =
    RIPPLE_WAVELENGTH * RIPPLE_WAVELENGTH * rippleScale * rippleScale
  const phaseScale = (Math.PI * 2) / RIPPLE_WAVELENGTH
  const scaledAmplitude = RIPPLE_AMPLITUDE * rippleScale

  const rCx: number[] = []
  const rCy: number[] = []
  const rWaveFrontScaled: number[] = []
  const rTimeDecay: number[] = []
  const rElapsed: number[] = []
  const rMinRSq: number[] = []
  const rMaxRSq: number[] = []

  let globalMinX = width
  let globalMaxX = 0
  let globalMinY = height
  let globalMaxY = 0

  for (let i = 0; i < activeCount; i++) {
    const ripple = ripples[i]
    const elapsed = (now - ripple.startTime) / 1000
    if (elapsed > durationSec) continue

    hasActiveRipple = true

    const waveFront = elapsed * RIPPLE_SPEED
    const progress = elapsed / durationSec
    const timeDecay = 1 - progress * progress

    const cx = ripple.x | 0
    const cy = ripple.y | 0
    const minR = Math.max(0, (waveFront - waveWidth) * rippleScale)
    const maxR = (waveFront + waveWidth) * rippleScale

    rCx.push(cx)
    rCy.push(cy)
    rWaveFrontScaled.push(waveFront * rippleScale)
    rTimeDecay.push(timeDecay)
    rElapsed.push(elapsed)
    rMinRSq.push(minR * minR)
    rMaxRSq.push(maxR * maxR)

    const minX = Math.max(0, (cx - maxR) | 0)
    const maxX = Math.min(width - 1, (cx + maxR) | 0)
    const minY = Math.max(0, (cy - maxR) | 0)
    const maxY = Math.min(height - 1, (cy + maxR) | 0)

    if (minX < globalMinX) globalMinX = minX
    if (maxX > globalMaxX) globalMaxX = maxX
    if (minY < globalMinY) globalMinY = minY
    if (maxY > globalMaxY) globalMaxY = maxY
  }

  if (!hasActiveRipple) {
    ctx.putImageData(destImageData, 0, 0)
    return { hasActive: false, destData: destImageData }
  }

  const rippleLen = rCx.length

  for (let y = globalMinY; y <= globalMaxY; y++) {
    const rowOffset = y * width

    for (let x = globalMinX; x <= globalMaxX; x++) {
      let totalDx = 0
      let totalDy = 0

      for (let i = 0; i < rippleLen; i++) {
        const dx = x - rCx[i]
        const dy = y - rCy[i]
        const distSq = dx * dx + dy * dy

        if (distSq > rMaxRSq[i] || distSq < rMinRSq[i]) continue

        const distance = Math.sqrt(distSq)
        if (distance < 0.1) continue

        const distFromFront = distance - rWaveFrontScaled[i]
        const envelope = fastExp(
          -(distFromFront * distFromFront) / wavelengthSqScaled,
        )
        const phase = (distance * phaseScale) / rippleScale - rElapsed[i] * 10
        const wave = fastSin(phase)
        const strength = scaledAmplitude * envelope * rTimeDecay[i] * wave
        const invDist = 1 / distance

        totalDx += dx * invDist * strength
        totalDy += dy * invDist * strength
      }

      if (totalDx !== 0 || totalDy !== 0) {
        const srcX = (x - totalDx + 0.5) | 0
        const srcY = (y - totalDy + 0.5) | 0

        if (srcX >= 0 && srcX < width && srcY >= 0 && srcY < height) {
          dest32[rowOffset + x] = src32[srcY * width + srcX]
        }
      }
    }
  }

  ctx.putImageData(destImageData, 0, 0)
  return { hasActive: hasActiveRipple, destData: destImageData }
}

export function useEvocativeWallpaper(
  elementId = 'wallpaper',
  options: EvocativeOptions = {},
) {
  const {
    parallax = {},
    dynamicBlur = {},
    ripple = {},
    fps = 30,
    rippleQuality = 0.85,
  } = options

  const enableParallax = parallax.enabled ?? true
  const enableGyroscope = parallax.enableGyroscope ?? true
  const maxOffset = parallax.maxOffset ?? PARALLAX_MAX_OFFSET

  const enableDynamicBlur = dynamicBlur.enabled ?? false
  const baseBlur = dynamicBlur.baseBlur ?? 3
  const unblurZone = dynamicBlur.unblurZone ?? UNBLUR_ZONE

  const enableRipple = ripple.enabled ?? false

  const targetFps = fps >= 60 ? 60 : 30
  const rippleScale = Math.max(0.5, Math.min(1.0, rippleQuality))

  const stateRef = useRef<EvocativeState>({
    active: false,
    pageVisible: true,
    el: null,
    raf: null,
    returning: false,

    parallaxTx: 0,
    parallaxTy: 0,
    parallaxCx: 0,
    parallaxCy: 0,
    parallaxLastRx: 0,
    parallaxLastRy: 0,
    parallaxIdle: true,
    parallaxOffsetMult: maxOffset * 2,
    parallaxGyroMult: maxOffset * GYRO_SENS * 2,
    gyroEnabled: false,
    permissionRequested: false,
    reqHandler: null,

    blurTargetBlur: baseBlur,
    blurCurrentBlur: baseBlur,
    blurBaseBlur: baseBlur,
    blurLastRendered: baseBlur,
    blurIdle: true,

    rippleCanvas: null,
    rippleCtx: null,
    sourceImageData: null,
    destImageData: null,
    activeRipples: [],
    rippleRaf: null,
    rippleFadeoutTimer: null,
    rippleIsFadingOut: false,
  })

  // 仅「曾开启 → 全关 → 再开」才 soft-restore，避免首屏缩放
  const wasAnyEnabledRef = useRef(false)
  const everEnabledRef = useRef(false)

  useEffect(() => {
    const s = stateRef.current
    s.blurBaseBlur = baseBlur
    if (s.blurIdle) {
      s.blurTargetBlur = baseBlur
      s.blurCurrentBlur = baseBlur
      s.blurLastRendered = baseBlur
    }
  }, [baseBlur])

  useEffect(() => {
    const s = stateRef.current
    const anyEnabled = enableParallax || enableDynamicBlur || enableRipple

    if (!anyEnabled) {
      const el = document.getElementById(elementId)
      if (el) {
        // 进画布：各端 soft-lock 缓入 identity；inline 固定为 identity，
        // 离场摘掉 CSS 时不会弹回旧 parallax 位移。
        if (isLibraryCanvasHoldingWallpaper()) {
          softLockWallpaperTransform(el)
        } else {
          clearWallpaperTransformStyles(el)
        }
        el.style.willChange = ''
        // 注意：不清除 filter，因为基础模糊由 useWallpaper 管理
      }
      wasAnyEnabledRef.current = false
      return
    }

    const el = document.getElementById(elementId)
    if (!el) {
      console.warn('[EvocativeWallpaper] Element not found:', elementId)
      return
    }

    const softRestore = everEnabledRef.current && !wasAnyEnabledRef.current
    wasAnyEnabledRef.current = true
    everEnabledRef.current = true

    s.active = true
    s.pageVisible = isPageVisible()
    s.el = el
    s.raf = null
    s.returning = false

    s.parallaxTx = 0
    s.parallaxTy = 0
    s.parallaxCx = 0
    s.parallaxCy = 0
    s.parallaxLastRx = 0
    s.parallaxLastRy = 0
    s.parallaxIdle = true
    s.parallaxOffsetMult = maxOffset * 2
    s.parallaxGyroMult = maxOffset * GYRO_SENS * 2
    s.gyroEnabled = false
    s.permissionRequested = false

    s.blurTargetBlur = baseBlur
    s.blurCurrentBlur = baseBlur
    s.blurBaseBlur = baseBlur
    s.blurLastRendered = baseBlur
    s.blurIdle = true

    s.activeRipples = []
    s.rippleRaf = null

    el.style.transformOrigin = 'center'
    const willChangeProps: string[] = []
    if (enableParallax) willChangeProps.push('transform')
    if (enableDynamicBlur) willChangeProps.push('filter')
    el.style.willChange = willChangeProps.join(', ')

    let softRestoreTimer: ReturnType<typeof setTimeout> | null = null
    let softRestoreRaf1 = 0
    let softRestoreRaf2 = 0
    /** soft-restore 期间禁止交互改 transform，避免与 CSS 过渡互抢 */
    let interactionReady = !softRestore || !enableParallax

    if (enableParallax) {
      if (softRestore) {
        // Drop soft-lock's important transition so restore can interpolate.
        el.style.removeProperty('transition')
        el.style.setProperty('transition', EFFECT_EDGE_TRANSITION)
        el.style.transform = IDENTITY_TF
        // 双 rAF：先提交 identity，再过渡到 idle scale，保证浏览器能插值
        softRestoreRaf1 = requestAnimationFrame(() => {
          softRestoreRaf2 = requestAnimationFrame(() => {
            if (!s.active || s.el !== el) return
            el.style.transform = IDLE_TF
          })
        })
        softRestoreTimer = setTimeout(() => {
          if (!s.active || s.el !== el) return
          el.style.removeProperty('transition')
          interactionReady = true
          softRestoreTimer = null
        }, EFFECT_EDGE_MS)
      } else {
        el.style.removeProperty('transition')
        el.style.transform = IDLE_TF
      }
    }
    if (enableDynamicBlur) {
      el.style.filter = `${BLUR_PREFIX}${effectiveWallpaperBlur(baseBlur)}${BLUR_SUFFIX}`
    }

    if (enableRipple) {
      s.rippleCanvas = createRippleCanvas(el, enableParallax)
      s.rippleCtx = s.rippleCanvas.getContext('2d', {
        willReadFrequently: true,
      })
    }

    const motionClock = createFrameClock(targetFps)
    const rippleClock = createFrameClock(60)

    const rippleAnimationLoop = (now: number) => {
      if (!s.active || !s.rippleCanvas || !s.rippleCtx || !s.sourceImageData)
        return

      if (rippleClock.advance(now) === null) {
        s.rippleRaf = requestAnimationFrame(rippleAnimationLoop)
        return
      }

      if (s.el) {
        s.rippleCanvas.style.transform = buildCanvasTransform(
          s.el,
          enableParallax,
        )
      }

      const durationSec = RIPPLE_DURATION / 1000
      s.activeRipples = s.activeRipples.filter(
        (r) => (now - r.startTime) / 1000 <= durationSec,
      )

      const result = applyRippleDistortion(
        s.rippleCtx,
        s.sourceImageData,
        s.destImageData,
        s.rippleCanvas.width,
        s.rippleCanvas.height,
        s.activeRipples,
        now,
        rippleScale,
      )

      s.destImageData = result.destData

      if (result.hasActive) {
        s.rippleRaf = requestAnimationFrame(rippleAnimationLoop)
      } else {
        s.rippleRaf = null
        s.rippleIsFadingOut = true
        s.rippleCanvas.style.transition = 'opacity 0.75s ease-out'
        s.rippleCanvas.style.opacity = '0'

        if (s.rippleFadeoutTimer) {
          clearTimeout(s.rippleFadeoutTimer)
        }

        s.rippleFadeoutTimer = setTimeout(() => {
          // 只有在仍处于淡出状态时才清理（避免新涟漪被意外清理）
          if (s.rippleIsFadingOut) {
            s.activeRipples = []
            s.sourceImageData = null
            // 释放输出暂存 buffer（~6MB）；下次涟漪 applyRippleDistortion 会按需 createImageData 重建
            s.destImageData = null
            s.rippleIsFadingOut = false
            if (s.rippleCanvas) {
              s.rippleCanvas.style.transition = 'opacity 0.15s ease-out'
              // 已淡出不可见；释放视口大小的位图，下次 startRipple 按需重建
              s.rippleCanvas.width = 0
              s.rippleCanvas.height = 0
            }
          }
          s.rippleFadeoutTimer = null
        }, 750)
      }
    }

    const startRipple = async (x: number, y: number) => {
      if (!s.rippleCanvas || !s.rippleCtx || !s.el) return

      // Bitmap size tracks #bg-container (lvh crop), not window.inner*
      const { width: vw, height: vh } = getRippleViewportSize()
      const expectedWidth = (vw * rippleScale) | 0
      const expectedHeight = (vh * rippleScale) | 0
      if (
        s.rippleCanvas.width !== expectedWidth ||
        s.rippleCanvas.height !== expectedHeight
      ) {
        s.rippleCanvas.width = expectedWidth
        s.rippleCanvas.height = expectedHeight
        // 尺寸变化后需要重新捕获壁纸
        s.sourceImageData = null
        s.destImageData = null
      }

      if (s.rippleIsFadingOut) {
        s.rippleIsFadingOut = false
        if (s.rippleFadeoutTimer) {
          clearTimeout(s.rippleFadeoutTimer)
          s.rippleFadeoutTimer = null
        }
        s.rippleCanvas.style.transition = 'opacity 0.15s ease-out'
        s.rippleCanvas.style.opacity = '1'
      }

      // 如果没有活动涟漪，需要重新捕获壁纸
      if (s.activeRipples.length === 0 || !s.sourceImageData) {
        s.sourceImageData = await captureWallpaperToCanvas(
          s.el,
          s.rippleCanvas,
          s.rippleCtx,
          s.blurCurrentBlur,
          rippleScale,
        )
        if (!s.sourceImageData) return
        s.rippleCanvas.style.opacity = '1'
      }

      if (s.activeRipples.length >= MAX_RIPPLES) {
        s.activeRipples.shift()
      }

      s.activeRipples.push({
        x: x * rippleScale,
        y: y * rippleScale,
        startTime: performance.now(),
      })

      if (!s.rippleRaf) {
        rippleClock.reset(performance.now())
        s.rippleRaf = requestAnimationFrame(rippleAnimationLoop)
      }
    }

    const tick = (t: number) => {
      if (!s.active) return
      if (!s.pageVisible) {
        s.raf = null
        return
      }

      // Canvas marked active (layout) before evocative effect tears down —
      // stop writing parallax so soft-lock's cached from-frame can ease out.
      if (isLibraryCanvasHoldingWallpaper()) {
        softLockWallpaperTransform(el)
        s.parallaxIdle = true
        s.raf = null
        return
      }

      let delta = motionClock.advance(t)
      if (delta !== null) {
        if (delta > MAX_DELTA) delta = MAX_DELTA

        const parallaxSmooth = s.returning
          ? PARALLAX_SMOOTH_RETURN
          : PARALLAX_SMOOTH
        const blurSmooth = s.returning ? BLUR_SMOOTH_RETURN : BLUR_SMOOTH
        const parallaxFactor = 1 - (1 - parallaxSmooth) ** (delta / 16)
        const blurFactor = 1 - (1 - blurSmooth) ** (delta / 16)

        let needsContinue = false

        if (enableParallax && !s.parallaxIdle) {
          const dx = (s.parallaxTx - s.parallaxCx) * parallaxFactor
          const dy = (s.parallaxTy - s.parallaxCy) * parallaxFactor

          const remainingX = Math.abs(s.parallaxTx - s.parallaxCx)
          const remainingY = Math.abs(s.parallaxTy - s.parallaxCy)

          if (remainingX + remainingY < THRESHOLD) {
            s.parallaxIdle = true
            s.parallaxCx = s.parallaxTx
            s.parallaxCy = s.parallaxTy
            s.parallaxLastRx = Math.round(s.parallaxCx * 10) / 10
            s.parallaxLastRy = Math.round(s.parallaxCy * 10) / 10
            el.style.transform = buildTransform(s.parallaxCx, s.parallaxCy)
          } else {
            s.parallaxCx += dx
            s.parallaxCy += dy

            const rx = Math.round(s.parallaxCx * 10) / 10
            const ry = Math.round(s.parallaxCy * 10) / 10

            if (rx !== s.parallaxLastRx || ry !== s.parallaxLastRy) {
              s.parallaxLastRx = rx
              s.parallaxLastRy = ry
              el.style.transform = buildTransform(s.parallaxCx, s.parallaxCy)
            }
            needsContinue = true
          }
        }

        if (enableDynamicBlur && !s.blurIdle) {
          const diff = s.blurTargetBlur - s.blurCurrentBlur
          const diffAbs = diff < 0 ? -diff : diff

          if (diffAbs < THRESHOLD * 0.6) {
            s.blurIdle = true
            s.blurCurrentBlur = s.blurTargetBlur

            const currentAbs = s.blurCurrentBlur - s.blurLastRendered
            if ((currentAbs < 0 ? -currentAbs : currentAbs) > THRESHOLD * 0.6) {
              s.blurLastRendered = s.blurCurrentBlur
              const rounded = ((s.blurCurrentBlur * 10 + 0.5) | 0) / 10
              batchWrite(() => {
                if (s.el)
                  s.el.style.filter = `${BLUR_PREFIX}${effectiveWallpaperBlur(rounded)}${BLUR_SUFFIX}`
              })
            }
          } else {
            s.blurCurrentBlur += diff * blurFactor
            const rounded = ((s.blurCurrentBlur * 10 + 0.5) | 0) / 10

            if (rounded !== s.blurLastRendered) {
              s.blurLastRendered = rounded
              batchWrite(() => {
                if (s.el)
                  s.el.style.filter = `${BLUR_PREFIX}${effectiveWallpaperBlur(rounded)}${BLUR_SUFFIX}`
              })
            }
            needsContinue = true
          }
        }

        if (needsContinue) {
          s.raf = requestAnimationFrame(tick)
        } else {
          s.raf = null
        }
      } else {
        s.raf = requestAnimationFrame(tick)
      }
    }

    const wake = () => {
      if (!s.pageVisible) return
      if (isLibraryCanvasHoldingWallpaper()) return

      const parallaxNeedsWake = enableParallax && s.parallaxIdle
      const blurNeedsWake = enableDynamicBlur && s.blurIdle

      if ((parallaxNeedsWake || blurNeedsWake) && s.active) {
        if (parallaxNeedsWake) s.parallaxIdle = false
        if (blurNeedsWake) s.blurIdle = false

        if (!s.raf) {
          motionClock.reset(performance.now())
          s.raf = requestAnimationFrame(tick)
        }
      }
    }

    const unsubscribeVisibility = onVisibility((visible) => {
      s.pageVisible = visible
      if (!visible) {
        if (s.raf !== null) cancelAnimationFrame(s.raf)
        s.raf = null
        if (s.rippleRaf !== null) cancelAnimationFrame(s.rippleRaf)
        s.rippleRaf = null
        return
      }
      if (s.active) {
        const needsResume =
          (enableParallax && !s.parallaxIdle) ||
          (enableDynamicBlur && !s.blurIdle)
        if (needsResume && !s.raf) {
          motionClock.reset(performance.now())
          s.raf = requestAnimationFrame(tick)
        }
        if (s.activeRipples.length > 0 && s.rippleRaf === null) {
          rippleClock.reset(performance.now())
          s.rippleRaf = requestAnimationFrame(rippleAnimationLoop)
        }
      }
    })

    // Input only updates targets; the frame loop coalesces DOM writes. Keep the
    // final event in a burst so a stationary pointer never leaves a stale target.
    // 空白判定要读 computed style，只在指针下的元素变化时重算。
    let lastBlankTarget: EventTarget | null = null
    let lastBlank = false
    let unblurTimer: ReturnType<typeof setTimeout> | null = null
    const clearUnblurTimer = () => {
      if (unblurTimer !== null) {
        clearTimeout(unblurTimer)
        unblurTimer = null
      }
    }

    const onMouseMove = (e: MouseEvent) => {
      if (!s.pageVisible || !interactionReady) return
      if (s.gyroEnabled) return

      s.returning = false

      if (enableParallax) {
        s.parallaxTx = -(e.clientX / innerWidth - 0.5) * s.parallaxOffsetMult
        s.parallaxTy = -(e.clientY / innerHeight - 0.5) * s.parallaxOffsetMult
      }

      if (enableDynamicBlur && e.target !== lastBlankTarget) {
        lastBlankTarget = e.target
        const blank = isPointerOverWallpaperBlank(e.target)
        if (blank !== lastBlank) {
          lastBlank = blank
          clearUnblurTimer()
          if (blank) {
            unblurTimer = setTimeout(() => {
              unblurTimer = null
              if (!s.active) return
              s.blurTargetBlur = 0
              wake()
            }, UNBLUR_DELAY_MS)
          } else {
            s.blurTargetBlur = s.blurBaseBlur
          }
        }
      }

      wake()
    }

    const onMouseLeave = (e: MouseEvent) => {
      if (!interactionReady) return
      // Fixed chrome (nav idle-hide) toggling pointer-events can synthesize
      // mouseleave while the cursor is still in the viewport.
      if (!isWallpaperMouseLeaveFromViewport(e.relatedTarget)) return
      s.returning = true

      if (enableParallax && !s.gyroEnabled) {
        s.parallaxTx = s.parallaxTy = 0
      }

      if (enableDynamicBlur) {
        clearUnblurTimer()
        lastBlankTarget = null
        lastBlank = false
        s.blurTargetBlur = s.blurBaseBlur
      }

      wake()
    }

    const onClick = (e: MouseEvent) => {
      if (
        !enableRipple ||
        !s.rippleCanvas ||
        !s.pageVisible ||
        !interactionReady
      ) {
        return
      }

      // Container-local coords so click origin matches the visible crop bitmap
      const viewportRect = getRippleViewportRect()
      const localX = e.clientX - viewportRect.left
      const localY = e.clientY - viewportRect.top
      const normalizedY =
        viewportRect.height > 0 ? localY / viewportRect.height : 0
      if (normalizedY > unblurZone) return

      const target = e.target as HTMLElement | null
      if (!target) return

      if (
        target.closest(
          'button, a, input, select, textarea, label, ' +
            '[role="button"], [role="link"], [role="checkbox"], [role="radio"], [role="switch"], [role="tab"], [role="menuitem"], [role="option"], [role="slider"], ' +
            '[role="dialog"], [role="menu"], [role="listbox"], [role="tooltip"], ' +
            '[tabindex]:not([tabindex="-1"]), ' +
            'nav, .nav-item, .nav-container, .dynamic-island, ' +
            '.card, .modal, .dialog, .dropdown, .menu, .popup, .tooltip, .toast, .panel, ' +
            'video, audio, iframe, ' +
            '[data-no-ripple]',
        )
      ) {
        return
      }

      startRipple(localX, localY)
    }

    const onGyro = (e: DeviceOrientationEvent) => {
      if (!s.pageVisible || !interactionReady) return
      const beta = e.beta
      const gamma = e.gamma
      if (beta == null || gamma == null) return

      const b = (beta < -45 ? -45 : beta > 45 ? 45 : beta) / 45
      const g = (gamma < -45 ? -45 : gamma > 45 ? 45 : gamma) / 45

      s.parallaxTx = -g * s.parallaxGyroMult
      s.parallaxTy = -b * s.parallaxGyroMult
      wake()
    }

    const isMobileOnly = window.matchMedia(
      '(hover: none) and (pointer: coarse)',
    ).matches

    if (!isMobileOnly) {
      window.addEventListener('mousemove', onMouseMove, { passive: true })
      document.documentElement.addEventListener('mouseleave', onMouseLeave)

      if (enableRipple) {
        window.addEventListener('click', onClick, { passive: true })
      }
    }

    let gyroProbeCancelled = false
    let gyroProbeTimer: number | null = null
    let testGyro: ((e: DeviceOrientationEvent) => void) | null = null

    // Hover devices must keep mouse parallax/blur. A Mac that emits
    // deviceorientation would otherwise set gyroEnabled and drop mousemove.
    const preferMouse = window.matchMedia('(hover: hover)').matches

    if (
      enableParallax &&
      enableGyroscope &&
      !preferMouse &&
      'DeviceOrientationEvent' in window
    ) {
      const DOE = DeviceOrientationEvent as {
        requestPermission?: () => Promise<string>
      }

      if (typeof DOE.requestPermission === 'function') {
        const requestPermission = async () => {
          if (s.permissionRequested || !s.active) return
          s.permissionRequested = true

          try {
            const permission = await DOE.requestPermission!()
            if (permission === 'granted' && s.active) {
              s.gyroEnabled = true
              window.addEventListener('deviceorientation', onGyro, {
                passive: true,
              })
            }
          } catch {
            s.permissionRequested = false
          }
        }

        s.reqHandler = requestPermission
        document.addEventListener('click', requestPermission)
        document.addEventListener('touchend', requestPermission)
      } else {
        let received = false
        testGyro = (e: DeviceOrientationEvent) => {
          if (gyroProbeCancelled) return
          if (e.beta != null && e.gamma != null) {
            received = true
            s.gyroEnabled = true
            if (testGyro) {
              window.removeEventListener('deviceorientation', testGyro)
              testGyro = null
            }
            window.addEventListener('deviceorientation', onGyro, {
              passive: true,
            })
          }
        }
        window.addEventListener('deviceorientation', testGyro, {
          passive: true,
        })
        gyroProbeTimer = window.setTimeout(() => {
          gyroProbeTimer = null
          if (!received && testGyro) {
            window.removeEventListener('deviceorientation', testGyro)
            testGyro = null
          }
        }, 3000)
      }
    }

    return () => {
      s.active = false
      gyroProbeCancelled = true
      if (gyroProbeTimer != null) {
        window.clearTimeout(gyroProbeTimer)
        gyroProbeTimer = null
      }
      if (testGyro) {
        window.removeEventListener('deviceorientation', testGyro)
        testGyro = null
      }
      if (s.raf) cancelAnimationFrame(s.raf)
      if (s.rippleRaf) cancelAnimationFrame(s.rippleRaf)
      if (s.rippleFadeoutTimer) clearTimeout(s.rippleFadeoutTimer)
      if (softRestoreTimer) clearTimeout(softRestoreTimer)
      if (softRestoreRaf1) cancelAnimationFrame(softRestoreRaf1)
      if (softRestoreRaf2) cancelAnimationFrame(softRestoreRaf2)
      clearUnblurTimer()

      unsubscribeVisibility()

      window.removeEventListener('mousemove', onMouseMove)
      document.documentElement.removeEventListener('mouseleave', onMouseLeave)
      window.removeEventListener('click', onClick)
      window.removeEventListener('deviceorientation', onGyro)

      if (s.reqHandler) {
        document.removeEventListener('click', s.reqHandler)
        document.removeEventListener('touchend', s.reqHandler)
        s.reqHandler = null
      }

      if (s.rippleCanvas) {
        s.rippleCanvas.width = 0
        s.rippleCanvas.height = 0
        s.rippleCanvas.parentNode?.removeChild(s.rippleCanvas)
        s.rippleCanvas = null
        s.rippleCtx = null
      }
      s.sourceImageData = null
      s.destImageData = null
      s.activeRipples = []

      // 资料库画布激活时：各端 soft-lock 缓入 identity；离场时不会弹回旧 parallax 位移。
      if (isLibraryCanvasHoldingWallpaper()) {
        softLockWallpaperTransform(el)
        el.style.willChange = ''
      } else {
        // 重绑 / 卸载时清掉 soft-lock/restore 的 transition，避免残留影响下一次
        clearWallpaperTransformStyles(el)
        el.style.willChange = ''
      }
      // 注意：不清除 filter，因为基础模糊由 useWallpaper 管理
    }
  }, [
    enableParallax,
    enableDynamicBlur,
    enableRipple,
    enableGyroscope,
    baseBlur,
    maxOffset,
    unblurZone,
    elementId,
    // 配置保存改 FPS / 涟漪画质后需重绑（不改动效算法，只重挂监听）
    targetFps,
    rippleScale,
  ])
}
