/**
 * Evocative 壁纸动效 Hook
 *
 * 统一管理三大壁纸特效：
 * 1. 微动 (Parallax) - 壁纸根据鼠标/陀螺仪产生轻微位移，创造立体空间感
 * 2. 动态模糊 (Dynamic Blur) - 鼠标在上方40%区域时解除模糊，下方60%恢复模糊
 * 3. 涟漪 (Ripple) - 在上方40%区域点击产生水波纹扩散效果
 *
 * 性能优化：
 * - 共享鼠标事件处理，减少事件监听器
 * - 统一动画循环，减少 RAF 调用
 * - 共享页面可见性检测
 * - 合并状态管理，减少内存占用
 * - 预计算查找表（正弦/指数）
 * - Canvas 降采样（85%）减少像素计算
 * - 只处理涟漪影响范围内的像素
 * - 使用 Uint32Array 批量读写像素
 *
 * @module useEvocativeWallpaper
 */

import { useEffect, useRef } from 'react'
import { effectiveWallpaperBlur } from '../utils/wallpaperState'
import { batchWrite, isPageVisible, onVisibility } from './animation/core'

// ==================== 共享配置常量 ====================
const SMOOTH = 0.08 // 基础平滑因子
const SMOOTH_RETURN = 0.04 // 归正时使用更慢的速度
const MAX_DELTA = 100 // 最大时间间隔
const THROTTLE_MS = 50 // 鼠标事件节流
const THRESHOLD = 0.05 // 静止检测阈值

// ==================== 微动配置 ====================
const PARALLAX_SCALE = 1.02
const PARALLAX_MAX_OFFSET = 8
const GYRO_SENS = 0.5

// 预计算的静态 transform 字符串
const STATIC_TF_PREFIX = `scale(${PARALLAX_SCALE}) translate3d(`
const STATIC_TF_SUFFIX = ',0)'
const IDLE_TF = `scale(${PARALLAX_SCALE}) translate3d(0,0,0)`

// ==================== 动态模糊配置 ====================
const UNBLUR_ZONE = 0.4 // 上方 40% 为解除模糊区域
const BLUR_PREFIX = 'blur('
const BLUR_SUFFIX = 'px)'

// ==================== 涟漪配置 ====================
const MAX_RIPPLES = 3
const RIPPLE_DURATION = 2000
const RIPPLE_SPEED = 400
const RIPPLE_WAVELENGTH = 80
const RIPPLE_AMPLITUDE = 15
const RIPPLE_FRAME_MS = 17

// 预计算正弦查找表
const SIN_TABLE_SIZE = 1024
const SIN_TABLE = new Float32Array(SIN_TABLE_SIZE)
for (let i = 0; i < SIN_TABLE_SIZE; i++) {
  SIN_TABLE[i] = Math.sin((i / SIN_TABLE_SIZE) * Math.PI * 2)
}

// 预计算指数衰减查找表
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

// ==================== 类型定义 ====================
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
  /** 微动效果配置 */
  parallax?: ParallaxOptions
  /** 动态模糊配置 */
  dynamicBlur?: DynamicBlurOptions
  /** 涟漪效果配置 */
  ripple?: RippleOptions
  /** 动效帧率 (30 或 60) */
  fps?: number
  /** 涟漪画质 (0.5-1.0) */
  rippleQuality?: number
}

interface EvocativeState {
  // 共享状态
  active: boolean
  pageVisible: boolean
  el: HTMLElement | null
  raf: number | null
  lastTime: number
  returning: boolean

  // 微动状态
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

  // 动态模糊状态
  blurTargetBlur: number
  blurCurrentBlur: number
  blurBaseBlur: number
  blurLastRendered: number
  blurIdle: boolean

  // 涟漪状态
  rippleCanvas: HTMLCanvasElement | null
  rippleCtx: CanvasRenderingContext2D | null
  sourceImageData: ImageData | null
  destImageData: ImageData | null
  activeRipples: Array<{ x: number; y: number; startTime: number }>
  rippleRaf: number | null
  lastRippleFrameTime: number
  rippleFadeoutTimer: ReturnType<typeof setTimeout> | null
  rippleIsFadingOut: boolean
}

// ==================== 工具函数 ====================

function buildTransform(cx: number, cy: number): string {
  const rx = ((cx * 10 + 0.5) | 0) / 10
  const ry = ((cy * 10 + 0.5) | 0) / 10
  return `${STATIC_TF_PREFIX}${rx}px,${ry}px${STATIC_TF_SUFFIX}`
}

function buildCanvasTransform(
  wallpaperEl: HTMLElement,
  parallaxEnabled: boolean,
): string {
  if (!parallaxEnabled) {
    // 微动关闭时不需要任何 transform 补偿
    return 'none'
  }
  const transform = wallpaperEl.style.transform
  if (!transform) return IDLE_TF
  const match = transform.match(/translate3d\([^)]+\)/)
  return match ? `scale(${PARALLAX_SCALE}) ${match[0]}` : IDLE_TF
}

function createRippleCanvas(
  wallpaperEl: HTMLElement,
  parallaxEnabled: boolean,
  rippleScale: number,
): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  canvas.id = 'wallpaper-ripple-canvas'
  canvas.width = (window.innerWidth * rippleScale) | 0
  canvas.height = (window.innerHeight * rippleScale) | 0

  const computedStyle = window.getComputedStyle(wallpaperEl)
  const transformOrigin = parallaxEnabled
    ? computedStyle.transformOrigin || 'center'
    : 'center'
  const canvasTransform = buildCanvasTransform(wallpaperEl, parallaxEnabled)

  // z-index:1 — 夹在 #wallpaper 与 #bg-gradient(z-2) 之间，保证涟漪可见且不挡底部遮罩
  canvas.style.cssText = `
    position: absolute;
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
      const imgRatio = img.width / img.height
      const canvasRatio = canvasW / canvasH

      let drawW: number, drawH: number

      if (imgRatio > canvasRatio) {
        drawH = canvasH
        drawW = drawH * imgRatio
      } else {
        drawW = canvasW
        drawH = drawW / imgRatio
      }

      const drawX = (canvasW - drawW) / 2
      const drawY = (canvasH - drawH) / 2

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
    destData && destData.width === width && destData.height === height
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

// ==================== 主 Hook ====================

/**
 * Evocative 壁纸动效 Hook
 * 统一管理微动、动态模糊、涟漪三大特效
 */
export function useEvocativeWallpaper(
  elementId = 'wallpaper',
  options: EvocativeOptions = {},
) {
  // 解构嵌套配置
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

  // 根据 fps 配置计算帧间隔
  const frameMs = fps >= 60 ? 17 : 33 // 60fps = 17ms, 30fps = 33ms
  // 根据 rippleQuality 配置计算 Canvas 缩放（限制在 0.5-1.0 范围）
  const rippleScale = Math.max(0.5, Math.min(1.0, rippleQuality))

  const stateRef = useRef<EvocativeState>({
    active: false,
    pageVisible: true,
    el: null,
    raf: null,
    lastTime: 0,
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
    lastRippleFrameTime: 0,
    rippleFadeoutTimer: null,
    rippleIsFadingOut: false,
  })

  // 当 baseBlur 变化时更新状态
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
        el.style.transform = ''
        el.style.transformOrigin = ''
        el.style.willChange = ''
        // 注意：不清除 filter，因为基础模糊由 useWallpaper 管理
      }
      return
    }

    const el = document.getElementById(elementId)
    if (!el) {
      console.warn('[EvocativeWallpaper] Element not found:', elementId)
      return
    }

    // 初始化共享状态
    s.active = true
    s.pageVisible = isPageVisible()
    s.el = el
    s.raf = null
    s.lastTime = 0
    s.returning = false

    // 初始化微动状态
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

    // 初始化模糊状态
    s.blurTargetBlur = baseBlur
    s.blurCurrentBlur = baseBlur
    s.blurBaseBlur = baseBlur
    s.blurLastRendered = baseBlur
    s.blurIdle = true

    // 初始化涟漪状态
    s.activeRipples = []
    s.rippleRaf = null
    s.lastRippleFrameTime = 0

    // 设置元素样式
    el.style.transformOrigin = 'center'
    const willChangeProps: string[] = []
    if (enableParallax) willChangeProps.push('transform')
    if (enableDynamicBlur) willChangeProps.push('filter')
    el.style.willChange = willChangeProps.join(', ')

    if (enableParallax) {
      el.style.transform = IDLE_TF
    }
    if (enableDynamicBlur) {
      el.style.filter = `${BLUR_PREFIX}${effectiveWallpaperBlur(baseBlur)}${BLUR_SUFFIX}`
    }

    // 创建涟漪 Canvas
    if (enableRipple) {
      s.rippleCanvas = createRippleCanvas(el, enableParallax, rippleScale)
      s.rippleCtx = s.rippleCanvas.getContext('2d', {
        willReadFrequently: true,
      })
    }

    // ==================== 涟漪动画循环 ====================
    const rippleAnimationLoop = (now: number) => {
      if (!s.active || !s.rippleCanvas || !s.rippleCtx || !s.sourceImageData)
        return

      const deltaFrame = now - s.lastRippleFrameTime
      if (deltaFrame < RIPPLE_FRAME_MS) {
        s.rippleRaf = requestAnimationFrame(rippleAnimationLoop)
        return
      }
      s.lastRippleFrameTime = now

      if (s.el) {
        s.rippleCanvas.style.transform = buildCanvasTransform(
          s.el,
          enableParallax,
        )
      }

      // 清理已过期的涟漪（超过 RIPPLE_DURATION）
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
        // 涟漪结束，开始淡出
        s.rippleRaf = null
        s.rippleIsFadingOut = true
        s.rippleCanvas.style.transition = 'opacity 0.75s ease-out'
        s.rippleCanvas.style.opacity = '0'

        // 清除之前的淡出计时器（如果有）
        if (s.rippleFadeoutTimer) {
          clearTimeout(s.rippleFadeoutTimer)
        }

        // 等待淡出动画完成后再清理状态
        s.rippleFadeoutTimer = setTimeout(() => {
          // 只有在仍处于淡出状态时才清理（避免新涟漪被意外清理）
          if (s.rippleIsFadingOut) {
            s.activeRipples = []
            s.sourceImageData = null
            // 释放输出暂存 buffer（~6MB）；下次涟漪 applyRippleDistortion 会按需 createImageData 重建
            s.destImageData = null
            s.rippleIsFadingOut = false
            // 恢复快速淡入的 transition
            if (s.rippleCanvas) {
              s.rippleCanvas.style.transition = 'opacity 0.15s ease-out'
            }
          }
          s.rippleFadeoutTimer = null
        }, 750)
      }
    }

    const startRipple = async (x: number, y: number) => {
      if (!s.rippleCanvas || !s.rippleCtx || !s.el) return

      // 检查并更新 canvas 尺寸（窗口可能已调整大小）
      const expectedWidth = (window.innerWidth * rippleScale) | 0
      const expectedHeight = (window.innerHeight * rippleScale) | 0
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

      // 如果正在淡出，取消淡出并重新激活
      if (s.rippleIsFadingOut) {
        s.rippleIsFadingOut = false
        if (s.rippleFadeoutTimer) {
          clearTimeout(s.rippleFadeoutTimer)
          s.rippleFadeoutTimer = null
        }
        // 立即显示 Canvas（取消淡出动画）
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

      // 检查涟漪数量限制（移除最早的涟漪以腾出空间）
      if (s.activeRipples.length >= MAX_RIPPLES) {
        s.activeRipples.shift()
      }

      s.activeRipples.push({
        x: x * rippleScale,
        y: y * rippleScale,
        startTime: performance.now(),
      })

      if (!s.rippleRaf) {
        s.lastRippleFrameTime = performance.now()
        s.rippleRaf = requestAnimationFrame(rippleAnimationLoop)
      }
    }

    // ==================== 统一动画循环 ====================
    const tick = (t: number) => {
      if (!s.active) return
      if (!s.pageVisible) {
        s.raf = null
        return
      }

      let delta = t - s.lastTime
      if (delta >= frameMs) {
        if (delta > MAX_DELTA) delta = MAX_DELTA

        const smoothFactor = s.returning ? SMOOTH_RETURN : SMOOTH
        const factor = delta * smoothFactor * 0.0625

        let needsContinue = false

        // ========== 微动更新 ==========
        if (enableParallax && !s.parallaxIdle) {
          const dx = (s.parallaxTx - s.parallaxCx) * factor
          const dy = (s.parallaxTy - s.parallaxCy) * factor

          const txAbs = s.parallaxTx < 0 ? -s.parallaxTx : s.parallaxTx
          const tyAbs = s.parallaxTy < 0 ? -s.parallaxTy : s.parallaxTy
          const dxAbs = dx < 0 ? -dx : dx
          const dyAbs = dy < 0 ? -dy : dy

          if (txAbs + tyAbs < THRESHOLD && dxAbs + dyAbs < THRESHOLD) {
            s.parallaxIdle = true
            s.parallaxCx = s.parallaxCy = 0
            if (s.parallaxLastRx !== 0 || s.parallaxLastRy !== 0) {
              s.parallaxLastRx = s.parallaxLastRy = 0
              el.style.transform = IDLE_TF
            }
          } else {
            s.parallaxCx += dx
            s.parallaxCy += dy

            const rx = ((s.parallaxCx * 10 + 0.5) | 0) / 10
            const ry = ((s.parallaxCy * 10 + 0.5) | 0) / 10

            if (rx !== s.parallaxLastRx || ry !== s.parallaxLastRy) {
              s.parallaxLastRx = rx
              s.parallaxLastRy = ry
              el.style.transform = buildTransform(s.parallaxCx, s.parallaxCy)
            }
            needsContinue = true
          }
        }

        // ========== 模糊更新 ==========
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
            s.blurCurrentBlur += diff * factor
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

        s.lastTime = t

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

      const parallaxNeedsWake = enableParallax && s.parallaxIdle
      const blurNeedsWake = enableDynamicBlur && s.blurIdle

      if ((parallaxNeedsWake || blurNeedsWake) && s.active) {
        if (parallaxNeedsWake) s.parallaxIdle = false
        if (blurNeedsWake) s.blurIdle = false

        if (!s.raf) {
          s.lastTime = performance.now()
          s.raf = requestAnimationFrame(tick)
        }
      }
    }

    // ==================== 页面可见性 ====================
    const unsubscribeVisibility = onVisibility((visible) => {
      s.pageVisible = visible
      if (visible && s.active) {
        const needsResume =
          (enableParallax && !s.parallaxIdle) ||
          (enableDynamicBlur && !s.blurIdle)
        if (needsResume && !s.raf) {
          s.lastTime = performance.now()
          s.raf = requestAnimationFrame(tick)
        }
      }
    })

    // ==================== 鼠标事件 ====================
    let lastMouseTime = 0
    const onMouseMove = (e: MouseEvent) => {
      if (!s.pageVisible) return
      if (s.gyroEnabled) return

      const now = performance.now()
      if (now - lastMouseTime < THROTTLE_MS) return
      lastMouseTime = now

      s.returning = false

      // 微动计算
      if (enableParallax) {
        s.parallaxTx = -(e.clientX / innerWidth - 0.5) * s.parallaxOffsetMult
        s.parallaxTy = -(e.clientY / innerHeight - 0.5) * s.parallaxOffsetMult
      }

      // 模糊计算
      if (enableDynamicBlur) {
        const normalizedY = e.clientY / window.innerHeight
        if (normalizedY <= unblurZone) {
          s.blurTargetBlur = s.blurBaseBlur * (normalizedY / unblurZone)
        } else {
          s.blurTargetBlur = s.blurBaseBlur
        }
      }

      wake()
    }

    const onMouseLeave = () => {
      s.returning = true

      if (enableParallax && !s.gyroEnabled) {
        s.parallaxTx = s.parallaxTy = 0
      }

      if (enableDynamicBlur) {
        s.blurTargetBlur = s.blurBaseBlur
      }

      wake()
    }

    // ==================== 点击涟漪 ====================
    const onClick = (e: MouseEvent) => {
      if (!enableRipple || !s.rippleCanvas || !s.pageVisible) return

      const normalizedY = e.clientY / window.innerHeight
      if (normalizedY > unblurZone) return

      const target = e.target as HTMLElement | null
      if (!target) return

      // 排除交互元素
      if (
        target.closest(
          // 基础交互元素
          'button, a, input, select, textarea, label, ' +
            // ARIA 交互角色
            '[role="button"], [role="link"], [role="checkbox"], [role="radio"], [role="switch"], [role="tab"], [role="menuitem"], [role="option"], [role="slider"], ' +
            // 弹出层
            '[role="dialog"], [role="menu"], [role="listbox"], [role="tooltip"], ' +
            // 可聚焦元素
            '[tabindex]:not([tabindex="-1"]), ' +
            // 导航相关
            'nav, .nav-item, .nav-container, .dynamic-island, ' +
            // 常见UI组件
            '.card, .modal, .dialog, .dropdown, .menu, .popup, .tooltip, .toast, .panel, ' +
            // 媒体和嵌入
            'video, audio, iframe, ' +
            // 自定义排除
            '[data-no-ripple]',
        )
      ) {
        return
      }

      startRipple(e.clientX, e.clientY)
    }

    // ==================== 陀螺仪 ====================
    let lastGyroTime = 0
    const onGyro = (e: DeviceOrientationEvent) => {
      const beta = e.beta
      const gamma = e.gamma
      if (beta == null || gamma == null) return

      const now = performance.now()
      if (now - lastGyroTime < THROTTLE_MS) return
      lastGyroTime = now

      const b = (beta < -45 ? -45 : beta > 45 ? 45 : beta) / 45
      const g = (gamma < -45 ? -45 : gamma > 45 ? 45 : gamma) / 45

      s.parallaxTx = -g * s.parallaxGyroMult
      s.parallaxTy = -b * s.parallaxGyroMult
      wake()
    }

    // ==================== 事件绑定 ====================
    const isMobileOnly = window.matchMedia(
      '(hover: none) and (pointer: coarse)',
    ).matches

    if (!isMobileOnly) {
      window.addEventListener('mousemove', onMouseMove, { passive: true })
      document.addEventListener('mouseleave', onMouseLeave)

      if (enableRipple) {
        window.addEventListener('click', onClick, { passive: true })
      }
    }

    // 陀螺仪设置
    if (
      enableParallax &&
      enableGyroscope &&
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
        const testGyro = (e: DeviceOrientationEvent) => {
          if (e.beta != null && e.gamma != null) {
            received = true
            s.gyroEnabled = true
            window.removeEventListener('deviceorientation', testGyro)
            window.addEventListener('deviceorientation', onGyro, {
              passive: true,
            })
          }
        }
        window.addEventListener('deviceorientation', testGyro, {
          passive: true,
        })
        setTimeout(() => {
          if (!received)
            window.removeEventListener('deviceorientation', testGyro)
        }, 3000)
      }
    }

    // ==================== 清理 ====================
    return () => {
      s.active = false
      if (s.raf) cancelAnimationFrame(s.raf)
      if (s.rippleRaf) cancelAnimationFrame(s.rippleRaf)
      if (s.rippleFadeoutTimer) clearTimeout(s.rippleFadeoutTimer)

      unsubscribeVisibility()

      window.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseleave', onMouseLeave)
      window.removeEventListener('click', onClick)
      window.removeEventListener('deviceorientation', onGyro)

      if (s.reqHandler) {
        document.removeEventListener('click', s.reqHandler)
        document.removeEventListener('touchend', s.reqHandler)
        s.reqHandler = null
      }

      if (s.rippleCanvas && s.rippleCanvas.parentNode) {
        s.rippleCanvas.parentNode.removeChild(s.rippleCanvas)
        s.rippleCanvas = null
        s.rippleCtx = null
      }

      el.style.transform = ''
      el.style.transformOrigin = ''
      el.style.willChange = ''
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
  ])
}

export default useEvocativeWallpaper
