/**
 * 颜色提取器
 *
 * 从图片中提取主色调配色方案
 * 支持壁纸和音乐封面两种场景
 *
 * 性能优化：
 * - 使用 Image 对象池减少 GC
 * - 使用 Canvas 对象池减少 DOM 创建
 *
 * @module colorExtractor
 * @version 3.3
 */

import { imagePool, withPooledCanvas } from './objectPool'
import { wallpaperState } from './wallpaperState'

// ============================================================================
// 类型定义
// ============================================================================

export interface ColorPalette {
  primary: string
  secondary: string
  accent: string
  light: string
  dark: string
}

interface CachedColorData {
  url: string
  palette: ColorPalette
  timestamp: number
  version: number
}

interface ExtractOptions {
  /** 强制刷新，忽略缓存 */
  forceRefresh?: boolean
  /** 提取上下文：wallpaper | music */
  context?: 'wallpaper' | 'music' | string
}

interface ColorInfo {
  r: number
  g: number
  b: number
  percentage: number
  saturation: number
  brightness: number
  chroma: number
}

// ============================================================================
// 常量配置
// ============================================================================

const CACHE_VERSION = 5
const CACHE_EXPIRY_MS = 30 * 24 * 60 * 60 * 1000 // 30天 (localStorage 长期缓存)
const MAX_CANVAS_SIZE = 150
const SAMPLE_STEP = 4
const COLOR_QUANTIZE_STEP = 16

// 颜色检测阈值
const COLOR_THRESHOLDS = {
  minSaturation: 0.35,
  minChroma: 50,
  minGrayDistance: 40,
  minBrightness: 30,
  maxBrightness: 225,
  minPercentage: 2,
  fallbackMinPercentage: 1,
} as const

// 默认配色（灰色系）
const DEFAULT_PALETTE: ColorPalette = Object.freeze({
  primary: '#6b7280',
  secondary: '#9ca3af',
  accent: '#4b5563',
  light: '#d1d5db',
  dark: '#374151',
})

// ============================================================================
// 缓存管理
// ============================================================================

const memoryCache = new Map<string, ColorPalette>()

/** 当前正在进行的提取任务 */
let currentExtractionController: AbortController | null = null
let currentExtractionUrl: string | null = null

/**
 * 获取localStorage缓存
 */
function getLocalStorageCache(url: string): ColorPalette | null {
  try {
    const cached = localStorage.getItem('wallpaperColorCache')
    if (!cached)
      return null

    const data: CachedColorData = JSON.parse(cached)
    if (data.version !== CACHE_VERSION) {
      localStorage.removeItem('wallpaperColorCache')
      return null
    }
    if (data.url !== url)
      return null
    if (Date.now() - data.timestamp > CACHE_EXPIRY_MS)
      return null

    return data.palette
  }
  catch {
    return null
  }
}

/**
 * 保存到localStorage缓存
 */
function saveToLocalStorage(url: string, palette: ColorPalette): void {
  try {
    const data: CachedColorData = {
      url,
      palette,
      timestamp: Date.now(),
      version: CACHE_VERSION,
    }
    localStorage.setItem('wallpaperColorCache', JSON.stringify(data))
  }
  catch {
    // 静默失败
  }
}

// ============================================================================
// 颜色计算函数
// ============================================================================

/** 计算感知亮度 */
function getPerceptualBrightness(r: number, g: number, b: number): number {
  return 0.299 * r + 0.587 * g + 0.114 * b
}

/** 计算饱和度 */
function getSaturation(r: number, g: number, b: number): number {
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  return max === 0 ? 0 : (max - min) / max
}

/** 计算色度 */
function getChroma(r: number, g: number, b: number): number {
  return Math.max(r, g, b) - Math.min(r, g, b)
}

/** 计算与灰色的距离 */
function getDistanceFromGray(r: number, g: number, b: number): number {
  const avg = (r + g + b) / 3
  return Math.sqrt((r - avg) ** 2 + (g - avg) ** 2 + (b - avg) ** 2)
}

/** 检测是否为鲜艳的彩色 */
function isVividColor(r: number, g: number, b: number): boolean {
  const { minSaturation, minChroma, minGrayDistance, minBrightness, maxBrightness } = COLOR_THRESHOLDS

  const brightness = getPerceptualBrightness(r, g, b)
  if (brightness < minBrightness || brightness > maxBrightness)
    return false

  if (getSaturation(r, g, b) < minSaturation)
    return false
  if (getChroma(r, g, b) < minChroma)
    return false
  if (getDistanceFromGray(r, g, b) < minGrayDistance)
    return false

  // 检查RGB值是否太接近（灰色特征）
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const mid = r + g + b - max - min
  if (max - mid < 20 && mid - min < 20)
    return false

  return true
}

/** RGB转十六进制 */
function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  const toHex = (v: number) => clamp(v).toString(16).padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

/** RGB转HSL */
function rgbToHsl(r: number, g: number, b: number): { h: number, s: number, l: number } {
  r /= 255; g /= 255; b /= 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  const l = (max + min) / 2

  let h = 0; let s = 0
  if (delta !== 0) {
    s = l > 0.5 ? delta / (2 - max - min) : delta / (max + min)
    if (max === r)
      h = ((g - b) / delta + (g < b ? 6 : 0)) / 6
    else if (max === g)
      h = ((b - r) / delta + 2) / 6
    else h = ((r - g) / delta + 4) / 6
  }
  return { h, s, l }
}

/** HSL转RGB */
function hslToRgb(h: number, s: number, l: number): { r: number, g: number, b: number } {
  if (s === 0) {
    const v = Math.round(l * 255)
    return { r: v, g: v, b: v }
  }

  const hue2rgb = (p: number, q: number, t: number) => {
    if (t < 0)
      t += 1
    if (t > 1)
      t -= 1
    if (t < 1 / 6)
      return p + (q - p) * 6 * t
    if (t < 1 / 2)
      return q
    if (t < 2 / 3)
      return p + (q - p) * (2 / 3 - t) * 6
    return p
  }

  const q = l < 0.5 ? l * (1 + s) : l + s - l * s
  const p = 2 * l - q

  return {
    r: Math.round(hue2rgb(p, q, h + 1 / 3) * 255),
    g: Math.round(hue2rgb(p, q, h) * 255),
    b: Math.round(hue2rgb(p, q, h - 1 / 3) * 255),
  }
}

/** 生成亮色变体 */
function lightenColor(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b)
  hsl.l = Math.min(0.85, hsl.l + 0.2)
  hsl.s = Math.min(1, hsl.s * 1.1)
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return rgbToHex(rgb.r, rgb.g, rgb.b)
}

/** 生成暗色变体 */
function darkenColor(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b)
  hsl.l = Math.max(0.15, hsl.l - 0.25)
  hsl.s = Math.min(1, hsl.s * 1.15)
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return rgbToHex(rgb.r, rgb.g, rgb.b)
}

// ============================================================================
// 图片分析
// ============================================================================

/**
 * 分析图片颜色
 */
function analyzeImageColors(imageData: ImageData): ColorPalette {
  const pixels = imageData.data
  const colorMap = new Map<string, number>()
  let totalSamples = 0

  // 采样和量化
  for (let i = 0; i < pixels.length; i += SAMPLE_STEP * 4) {
    const r = pixels[i]
    const g = pixels[i + 1]
    const b = pixels[i + 2]
    const a = pixels[i + 3]

    if (a < 128)
      continue // 跳过透明像素
    if (!isVividColor(r, g, b))
      continue

    const qR = Math.round(r / COLOR_QUANTIZE_STEP) * COLOR_QUANTIZE_STEP
    const qG = Math.round(g / COLOR_QUANTIZE_STEP) * COLOR_QUANTIZE_STEP
    const qB = Math.round(b / COLOR_QUANTIZE_STEP) * COLOR_QUANTIZE_STEP
    const key = `${qR},${qG},${qB}`
    colorMap.set(key, (colorMap.get(key) || 0) + 1)
    totalSamples++
  }

  if (colorMap.size === 0) {
    return { ...DEFAULT_PALETTE }
  }

  // 排序并转换为颜色信息
  const sortedColors: ColorInfo[] = Array.from(colorMap.entries())
    .sort((a, b) => b[1] - a[1])
    .map(([color, count]) => {
      const [r, g, b] = color.split(',').map(Number)
      return {
        r,
        g,
        b,
        percentage: (count / totalSamples) * 100,
        saturation: getSaturation(r, g, b),
        brightness: getPerceptualBrightness(r, g, b),
        chroma: getChroma(r, g, b),
      }
    })

  // 筛选鲜艳颜色
  const { minPercentage, fallbackMinPercentage, minSaturation, minChroma } = COLOR_THRESHOLDS

  let selectedColors = sortedColors.filter(c =>
    c.percentage > minPercentage
    && c.saturation > minSaturation
    && c.chroma > minChroma
    && c.brightness > 40
    && c.brightness < 220,
  )

  // 如果没有足够鲜艳的颜色，放宽条件
  if (selectedColors.length === 0) {
    selectedColors = sortedColors.filter(c =>
      c.percentage > fallbackMinPercentage
      && c.saturation > minSaturation * 0.8
      && c.chroma > minChroma * 0.7,
    )
  }

  if (selectedColors.length === 0) {
    return { ...DEFAULT_PALETTE }
  }

  const primary = selectedColors[0]
  const secondary = selectedColors[1] || primary
  const accent = selectedColors[2] || secondary

  return {
    primary: rgbToHex(primary.r, primary.g, primary.b),
    secondary: rgbToHex(secondary.r, secondary.g, secondary.b),
    accent: rgbToHex(accent.r, accent.g, accent.b),
    light: lightenColor(primary.r, primary.g, primary.b),
    dark: darkenColor(primary.r, primary.g, primary.b),
  }
}

/**
 * 从图片提取颜色（使用对象池优化）
 */
async function extractFromImage(imageUrl: string, signal: AbortSignal): Promise<ColorPalette> {
  // 使用池化的 Image 对象
  const pooled = imagePool.acquire()
  const { img } = pooled
  img.crossOrigin = 'anonymous'

  // 注意：不添加 cacheBuster，因为浏览器缓存的图片可以直接使用
  // 添加 cacheBuster 会导致重新请求图片，增加延迟
  // 如果需要强制刷新，可以在 options 中传入 forceRefresh

  try {
    await new Promise<void>((resolve, reject) => {
      if (signal.aborted) {
        reject(new Error('Aborted before image load'))
        return
      }

      const abortHandler = () => reject(new Error('Aborted during image load'))
      signal.addEventListener('abort', abortHandler)

      img.onload = () => {
        signal.removeEventListener('abort', abortHandler)
        resolve()
      }
      img.onerror = () => {
        signal.removeEventListener('abort', abortHandler)
        reject(new Error('Failed to load image'))
      }
      img.src = imageUrl
    })

    if (signal.aborted) {
      throw new Error('Extraction cancelled after image load')
    }

    // 使用池化的 Canvas
    const scale = Math.min(MAX_CANVAS_SIZE / img.width, MAX_CANVAS_SIZE / img.height, 1)
    const width = Math.floor(img.width * scale)
    const height = Math.floor(img.height * scale)

    const imageData = withPooledCanvas(width, height, (ctx) => {
      ctx.drawImage(img, 0, 0, width, height)
      return ctx.getImageData(0, 0, width, height)
    })

    if (signal.aborted) {
      throw new Error('Extraction cancelled after processing')
    }

    return analyzeImageColors(imageData)
  }
  finally {
    // 确保归还 Image 到池中
    imagePool.release(pooled)
  }
}

// ============================================================================
// 公共 API
// ============================================================================

/**
 * 从图片提取颜色配色
 */
export async function extractColorsFromImage(
  imageUrl: string,
  options: ExtractOptions = {},
): Promise<ColorPalette> {
  const isMusic = options.context === 'music'
  const isWallpaper = options.context === 'wallpaper'

  // 非音乐提取时取消之前的任务
  if (!isMusic && currentExtractionController) {
    currentExtractionController.abort()
  }

  const myController = isMusic
    ? new AbortController()
    : (currentExtractionController = new AbortController())

  if (!isMusic) {
    currentExtractionUrl = imageUrl
  }

  try {
    // 壁纸提取时验证一致性（软验证，只记录警告）
    if (isWallpaper && !wallpaperState.isUrlActive(imageUrl)) {
      console.debug('[ColorExtractor] Wallpaper URL may have changed, but continuing extraction')
      // 不再抛出错误，继续提取（因为用户可能正在等待颜色）
    }

    // 检查缓存
    if (!options.forceRefresh) {
      const cached = memoryCache.get(imageUrl) || getLocalStorageCache(imageUrl)
      if (cached) {
        console.debug('[ColorExtractor] Using cached palette')
        memoryCache.set(imageUrl, cached)
        return cached
      }
    }

    console.debug('[ColorExtractor] Starting extraction for:', imageUrl.substring(0, 80))

    // 提取颜色
    const palette = await extractFromImage(imageUrl, myController.signal)

    if (myController.signal.aborted) {
      throw new Error('Extraction cancelled')
    }

    // 验证URL未变化
    if (!isMusic && currentExtractionUrl !== imageUrl) {
      console.debug('[ColorExtractor] URL changed during extraction, but using result anyway')
    }

    // 壁纸提取完成后验证（软验证）
    if (isWallpaper && !wallpaperState.isUrlActive(imageUrl)) {
      console.debug('[ColorExtractor] Wallpaper changed during extraction, but applying colors anyway')
    }

    // 缓存结果
    memoryCache.set(imageUrl, palette)
    saveToLocalStorage(imageUrl, palette)

    console.debug('[ColorExtractor] Extraction completed:', palette.primary)

    return palette
  }
  catch (error) {
    // 记录错误以便调试
    console.debug('[ColorExtractor] Extraction failed:', error instanceof Error ? error.message : error)

    if (error instanceof Error) {
      // 只在取消或壁纸变更时抛出错误
      if (error.message.includes('cancel') || error.message.includes('Wallpaper')) {
        throw error
      }
    }
    // 其他错误返回默认颜色
    return { ...DEFAULT_PALETTE }
  }
  finally {
    if (!isMusic && currentExtractionController === myController) {
      currentExtractionController = null
      currentExtractionUrl = null
    }
  }
}

/**
 * 应用颜色配色到CSS变量
 */
export function applyColorPalette(palette: ColorPalette): void {
  const root = document.documentElement
  root.style.setProperty('--color-primary', palette.primary)
  root.style.setProperty('--color-secondary', palette.secondary)
  root.style.setProperty('--color-accent', palette.accent)
  root.style.setProperty('--color-light', palette.light)
  root.style.setProperty('--color-dark', palette.dark)
}

/**
 * 清除颜色（重置为中性色）
 */
export function clearColors(): void {
  applyColorPalette({
    primary: '#94a3b8',
    secondary: '#94a3b8',
    accent: '#94a3b8',
    light: '#cbd5e1',
    dark: '#475569',
  })
}

/**
 * 清除颜色缓存
 */
export function clearColorCache(url?: string): void {
  if (url) {
    memoryCache.delete(url)
  }
  else {
    memoryCache.clear()
    localStorage.removeItem('wallpaperColorCache')
  }
}

/**
 * 从已加载的 HTMLImageElement 直接提取颜色
 * 用于处理跨域图片（如网站图标），因为已经渲染到页面的图片可以绑定到 canvas
 * 注意：如果图片跨域且服务器不支持 CORS，仍会失败
 */
export function extractColorsFromLoadedImage(img: HTMLImageElement): ColorPalette {
  try {
    // 使用池化的 Canvas
    const scale = Math.min(MAX_CANVAS_SIZE / img.naturalWidth, MAX_CANVAS_SIZE / img.naturalHeight, 1)
    const width = Math.floor(img.naturalWidth * scale) || 50
    const height = Math.floor(img.naturalHeight * scale) || 50

    const imageData = withPooledCanvas(width, height, (ctx) => {
      ctx.drawImage(img, 0, 0, width, height)
      return ctx.getImageData(0, 0, width, height)
    })

    return analyzeImageColors(imageData)
  }
  catch (err) {
    // 跨域图片会抛出安全错误
    console.debug('[ColorExtractor] Cannot extract from image (likely CORS):', err)
    return { ...DEFAULT_PALETTE }
  }
}

/**
 * 获取当前提取任务的URL
 */
export function getCurrentExtractionUrl(): string | null {
  return currentExtractionUrl
}

/**
 * 获取默认配色
 */
export function getDefaultPalette(): ColorPalette {
  return { ...DEFAULT_PALETTE }
}
