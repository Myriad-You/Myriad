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
 * @version 3.4
 */

import { syncCfgAccentColor } from './cfgAccent'
import {
  harmonizeGradientPalette,
  pickGradientCompanion,
} from './colorHarmony'
import { coverUrlForColorExtract } from './coverUrlForColorExtract'
import { imagePool, withPooledCanvas } from './objectPool'
import { wallpaperState } from './wallpaperState'

export { coverUrlForColorExtract } from './coverUrlForColorExtract'

// 类型定义

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
  /**
   * 仅 music 上下文：
   * - high（默认）：当前曲取色，会取消上一个 high 与所有 low 预取
   * - low：邻曲预取，不打断当前 high，彼此也可并行
   */
  priority?: 'high' | 'low'
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

// 常量配置

const CACHE_VERSION = 6
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

// 默认配色（灰色系）— 表示取色失败/无有效色，禁止当成功结果缓存
const DEFAULT_PALETTE: ColorPalette = Object.freeze({
  primary: '#6b7280',
  secondary: '#9ca3af',
  accent: '#4b5563',
  light: '#d1d5db',
  dark: '#374151',
})

/** 音乐 high 取色：瞬时失败（网络/解码）自动重试次数 */
const MUSIC_HIGH_MAX_ATTEMPTS = 3

// 缓存管理

const memoryCache = new Map<string, ColorPalette>()

/** 内存缓存最大条数（超出后 LRU 淘汰最旧项，防止无限增长） */
const MAX_MEMORY_CACHE = 50

/**
 * 是否为「取色失败占位」固定 DEFAULT 灰（网络/解码失败用）。
 * 注意：黑白封面分析出的真实灰阶 palette 不是它，应正常使用与缓存。
 */
export function isDefaultPalette(
  palette: ColorPalette | null | undefined,
): boolean {
  if (!palette) return true
  return (
    palette.primary === DEFAULT_PALETTE.primary &&
    palette.secondary === DEFAULT_PALETTE.secondary &&
    palette.accent === DEFAULT_PALETTE.accent &&
    palette.light === DEFAULT_PALETTE.light &&
    palette.dark === DEFAULT_PALETTE.dark
  )
}

/**
 * 写入内存缓存并维持 LRU 上限。
 * 命中失败时会回退 localStorage 或重算，因此淘汰是行为中性的。
 * 禁止写入默认灰，避免失败结果污染后续命中。
 */
function setMemoryCache(url: string, palette: ColorPalette): void {
  if (isDefaultPalette(palette)) return
  // 重新插入到末尾，使其成为「最近使用」
  memoryCache.delete(url)
  memoryCache.set(url, palette)
  while (memoryCache.size > MAX_MEMORY_CACHE) {
    const oldest = memoryCache.keys().next().value
    if (oldest === undefined) break
    memoryCache.delete(oldest)
  }
}

/** 当前正在进行的提取任务（壁纸 / 通用） */
let currentExtractionController: AbortController | null = null
let currentExtractionUrl: string | null = null
/** 音乐封面：当前曲 high 优先级取色 */
let musicHighController: AbortController | null = null
/** 当前 high 任务对应的封面 URL（用于判断是否应 abort） */
let musicHighUrl: string | null = null
/** 音乐封面：邻曲 low 优先级预取（可并行） */
const musicLowControllers = new Set<AbortController>()
/** 同一封面 URL 的 in-flight 去重（预取与当前曲撞同一图时复用） */
const musicInflight = new Map<string, Promise<ColorPalette>>()

function isAbortError(error: unknown): boolean {
  if (!(error instanceof Error)) return false
  const msg = error.message || ''
  return (
    error.name === 'AbortError' ||
    msg.includes('cancel') ||
    msg.includes('Abort') ||
    msg.includes('aborted')
  )
}

function sleepWithSignal(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new Error('Extraction cancelled'))
      return
    }
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    const onAbort = () => {
      clearTimeout(timer)
      signal.removeEventListener('abort', onAbort)
      reject(new Error('Extraction cancelled'))
    }
    signal.addEventListener('abort', onAbort)
  })
}

/**
 * 同步读内存调色板缓存（切歌热路径：避免再进 async extract）。
 * 默认灰视为未命中并剔除。
 */
export function getCachedPalette(url: string | null | undefined): ColorPalette | null {
  if (!url) return null
  const hit = memoryCache.get(url)
  if (!hit) return null
  if (isDefaultPalette(hit)) {
    memoryCache.delete(url)
    return null
  }
  // LRU touch
  setMemoryCache(url, hit)
  return hit
}

/**
 * 写入内存调色板（供播放器 colorCache 与 extractor 双写对齐）。
 */
export function setCachedPalette(
  url: string | null | undefined,
  palette: ColorPalette,
): void {
  if (!url || !palette || isDefaultPalette(palette)) return
  setMemoryCache(url, palette)
}

/**
 * 获取localStorage缓存
 */
function getLocalStorageCache(url: string): ColorPalette | null {
  try {
    const cached = localStorage.getItem('wallpaperColorCache')
    if (!cached) return null

    const data: CachedColorData = JSON.parse(cached)
    if (data.version !== CACHE_VERSION) {
      localStorage.removeItem('wallpaperColorCache')
      return null
    }
    if (data.url !== url) return null
    if (Date.now() - data.timestamp > CACHE_EXPIRY_MS) return null

    return data.palette
  } catch {
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
  } catch {
    // 静默失败
  }
}

// 颜色计算函数

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
  const {
    minSaturation,
    minChroma,
    minGrayDistance,
    minBrightness,
    maxBrightness,
  } = COLOR_THRESHOLDS

  const brightness = getPerceptualBrightness(r, g, b)
  if (brightness < minBrightness || brightness > maxBrightness) return false

  if (getSaturation(r, g, b) < minSaturation) return false
  if (getChroma(r, g, b) < minChroma) return false
  if (getDistanceFromGray(r, g, b) < minGrayDistance) return false

  // 检查RGB值是否太接近（灰色特征）
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const mid = r + g + b - max - min
  if (max - mid < 20 && mid - min < 20) return false

  return true
}

/** RGB转十六进制 */
function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  const toHex = (v: number) => clamp(v).toString(16).padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

/** RGB转HSL */
function rgbToHsl(
  r: number,
  g: number,
  b: number,
): { h: number; s: number; l: number } {
  r /= 255
  g /= 255
  b /= 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  const l = (max + min) / 2

  let h = 0
  let s = 0
  if (delta !== 0) {
    s = l > 0.5 ? delta / (2 - max - min) : delta / (max + min)
    if (max === r) h = ((g - b) / delta + (g < b ? 6 : 0)) / 6
    else if (max === g) h = ((b - r) / delta + 2) / 6
    else h = ((r - g) / delta + 4) / 6
  }
  return { h, s, l }
}

/** HSL转RGB */
function hslToRgb(
  h: number,
  s: number,
  l: number,
): { r: number; g: number; b: number } {
  if (s === 0) {
    const v = Math.round(l * 255)
    return { r: v, g: v, b: v }
  }

  const hue2rgb = (p: number, q: number, t: number) => {
    if (t < 0) t += 1
    if (t > 1) t -= 1
    if (t < 1 / 6) return p + (q - p) * 6 * t
    if (t < 1 / 2) return q
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
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

// 图片分析

/**
 * 从像素采样构建量化色直方图。
 * @param vividOnly 仅鲜艳色（音乐封面优先）；false 时纳入灰/低饱和（黑白封面）
 */
function sampleColorMap(
  pixels: Uint8ClampedArray | Uint8Array,
  vividOnly: boolean,
): { colorMap: Map<string, number>; totalSamples: number } {
  const colorMap = new Map<string, number>()
  let totalSamples = 0

  for (let i = 0; i < pixels.length; i += SAMPLE_STEP * 4) {
    const r = pixels[i]
    const g = pixels[i + 1]
    const b = pixels[i + 2]
    const a = pixels[i + 3]

    if (a < 128) continue
    if (vividOnly) {
      if (!isVividColor(r, g, b)) continue
    } else {
      // 中性色路径：跳过近全透明逻辑后的极端黑白噪声
      const br = getPerceptualBrightness(r, g, b)
      if (br < 12 || br > 244) continue
    }

    const qR = Math.round(r / COLOR_QUANTIZE_STEP) * COLOR_QUANTIZE_STEP
    const qG = Math.round(g / COLOR_QUANTIZE_STEP) * COLOR_QUANTIZE_STEP
    const qB = Math.round(b / COLOR_QUANTIZE_STEP) * COLOR_QUANTIZE_STEP
    const key = `${qR},${qG},${qB}`
    colorMap.set(key, (colorMap.get(key) || 0) + 1)
    totalSamples++
  }

  return { colorMap, totalSamples }
}

function paletteFromColorMap(
  colorMap: Map<string, number>,
  totalSamples: number,
  preferVivid: boolean,
): ColorPalette | null {
  if (colorMap.size === 0 || totalSamples <= 0) return null

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

  const { minPercentage, fallbackMinPercentage, minSaturation, minChroma } =
    COLOR_THRESHOLDS

  let selectedColors: ColorInfo[]

  if (preferVivid) {
    selectedColors = sortedColors.filter(
      (c) =>
        c.percentage > minPercentage &&
        c.saturation > minSaturation &&
        c.chroma > minChroma &&
        c.brightness > 40 &&
        c.brightness < 220,
    )
    if (selectedColors.length === 0) {
      selectedColors = sortedColors.filter(
        (c) =>
          c.percentage > fallbackMinPercentage &&
          c.saturation > minSaturation * 0.8 &&
          c.chroma > minChroma * 0.7,
      )
    }
  } else {
    // 灰阶/低饱和：按占比取主色，不强制 chroma/saturation
    selectedColors = sortedColors.filter(
      (c) =>
        c.percentage > fallbackMinPercentage &&
        c.brightness > 18 &&
        c.brightness < 235,
    )
    if (selectedColors.length === 0) {
      selectedColors = sortedColors.slice(0, 3)
    }
  }

  if (selectedColors.length === 0) return null

  const primary = selectedColors[0]
  const rest = selectedColors.slice(1)
  const secondary = pickGradientCompanion(primary, rest)
  const accent = pickGradientCompanion(
    primary,
    rest.filter(
      (c) =>
        c.r !== secondary.r || c.g !== secondary.g || c.b !== secondary.b,
    ),
  )

  return harmonizeGradientPalette({
    primary: rgbToHex(primary.r, primary.g, primary.b),
    secondary: rgbToHex(secondary.r, secondary.g, secondary.b),
    accent: rgbToHex(accent.r, accent.g, accent.b),
    light: lightenColor(primary.r, primary.g, primary.b),
    dark: darkenColor(primary.r, primary.g, primary.b),
  })
}

/**
 * 分析图片颜色。
 * 优先鲜艳色；无鲜艳色时（黑白/灰封面）回退中性采样，返回真实灰阶主题，
 * 而不是占位 DEFAULT_PALETTE（占位灰会被业务层当成失败丢弃）。
 */
function analyzeImageColors(imageData: ImageData): ColorPalette {
  const pixels = imageData.data

  const vivid = sampleColorMap(pixels, true)
  const vividPalette = paletteFromColorMap(
    vivid.colorMap,
    vivid.totalSamples,
    true,
  )
  if (vividPalette) return vividPalette

  const neutral = sampleColorMap(pixels, false)
  const neutralPalette = paletteFromColorMap(
    neutral.colorMap,
    neutral.totalSamples,
    false,
  )
  if (neutralPalette) return neutralPalette

  return { ...DEFAULT_PALETTE }
}

/**
 * 代理 soft-fail / 损坏图：1×1 透明 PNG 等。
 * 这类图 onload 成功但采样为空，会落成 DEFAULT 灰并被业务层丢弃。
 */
function isDegenerateImageSize(width: number, height: number): boolean {
  return (
    !Number.isFinite(width) ||
    !Number.isFinite(height) ||
    width <= 2 ||
    height <= 2
  )
}

/**
 * 从已解码像素分析；退化图直接抛错以便走 URL 回退 / 重试。
 */
function paletteFromRaster(
  width: number,
  height: number,
  draw: (ctx: CanvasRenderingContext2D) => void,
): ColorPalette {
  if (isDegenerateImageSize(width, height)) {
    throw new Error('Degenerate image (proxy placeholder or decode failure)')
  }

  const scale = Math.min(MAX_CANVAS_SIZE / width, MAX_CANVAS_SIZE / height, 1)
  const w = Math.max(1, Math.floor(width * scale))
  const h = Math.max(1, Math.floor(height * scale))

  let imageData: ImageData
  try {
    imageData = withPooledCanvas(w, h, (ctx) => {
      draw(ctx)
      return ctx.getImageData(0, 0, w, h)
    })
  } catch (err) {
    throw new Error(
      err instanceof Error
        ? `Canvas read failed: ${err.message}`
        : 'Canvas read failed (likely CORS)',
    )
  }

  const palette = analyzeImageColors(imageData)
  if (isDefaultPalette(palette)) {
    throw new Error('Empty color analysis (transparent or near-empty image)')
  }
  return palette
}

/**
 * 加载单张图并取色（对象池 Image）。
 * 注意：pool reset 会把 src 置空，避免「同 URL 不触发 onload」。
 */
async function extractFromSingleUrl(
  imageUrl: string,
  signal: AbortSignal,
): Promise<ColorPalette> {
  const pooled = imagePool.acquire()
  const { img } = pooled
  img.crossOrigin = 'anonymous'
  img.referrerPolicy = 'no-referrer'

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
        reject(new Error(`Failed to load image: ${imageUrl.slice(0, 120)}`))
      }

      // 强制与当前 src 不同，保证缓存命中时也重新走 load 事件
      if (img.src) {
        try {
          img.src = ''
        } catch {
          /* ignore */
        }
      }
      img.src = imageUrl
    })

    if (signal.aborted) {
      throw new Error('Extraction cancelled after image load')
    }

    const naturalW = img.naturalWidth || img.width
    const naturalH = img.naturalHeight || img.height

    return paletteFromRaster(naturalW, naturalH, (ctx) => {
      ctx.drawImage(img, 0, 0, ctx.canvas.width, ctx.canvas.height)
    })
  } finally {
    imagePool.release(pooled)
  }
}

/**
 * 从 URL 取色；可选 fallback（音乐：小尺寸失败 → 原始封面）。
 */
async function extractFromImage(
  imageUrl: string,
  signal: AbortSignal,
  fallbackUrl?: string | null,
): Promise<ColorPalette> {
  const candidates: string[] = [imageUrl]
  if (fallbackUrl && fallbackUrl !== imageUrl) {
    candidates.push(fallbackUrl)
  }

  let lastError: unknown = null
  for (let i = 0; i < candidates.length; i++) {
    if (signal.aborted) {
      throw new Error('Extraction cancelled')
    }
    try {
      return await extractFromSingleUrl(candidates[i], signal)
    } catch (error) {
      if (isAbortError(error)) throw error
      lastError = error
      console.debug(
        '[ColorExtractor] URL candidate failed:',
        i + 1,
        '/',
        candidates.length,
        error instanceof Error ? error.message : error,
      )
    }
  }

  throw lastError instanceof Error
    ? lastError
    : new Error('All image URL candidates failed')
}

// 公共 API

/**
 * 从图片提取颜色配色
 */
export async function extractColorsFromImage(
  imageUrl: string,
  options: ExtractOptions = {},
): Promise<ColorPalette> {
  const isMusic = options.context === 'music'
  const isWallpaper = options.context === 'wallpaper'
  const priority: 'high' | 'low' = options.priority ?? 'high'

  // ── 音乐路径：缓存 / in-flight 优先，再按 priority 调度 ──
  if (isMusic) {
    if (!options.forceRefresh) {
      const cached = getCachedPalette(imageUrl)
      if (cached) return cached
    }

    // 同封面 in-flight 去重：high/low 都 join，避免双请求互抢代理配额
    if (!options.forceRefresh) {
      const inflight = musicInflight.get(imageUrl)
      if (inflight) {
        if (priority === 'high') {
          // 切到该封面：取消「上一首」high（不同 URL），但不要 abort 本 URL 的 low
          if (
            musicHighController &&
            musicHighUrl &&
            musicHighUrl !== imageUrl
          ) {
            try {
              musicHighController.abort()
            } catch {
              /* ignore */
            }
            musicHighController = null
          }
          musicHighUrl = imageUrl
        }
        try {
          const reused = await inflight
          if (!isDefaultPalette(reused)) return reused
          // 预取失败得到 DEFAULT：high 必须自己再跑满重试；low 直接返回
          if (priority === 'low') return reused
        } catch (error) {
          if (isAbortError(error)) throw error
          if (priority === 'low') return { ...DEFAULT_PALETTE }
          console.debug(
            '[ColorExtractor] Music inflight failed, high will retry:',
            error instanceof Error ? error.message : error,
          )
        }
        // high + DEFAULT/失败 → fall through 新建 high（force 语义，不 join 旧 promise）
      }
    }

    // high：取消上一首 high + 其它封面的 low 预取（带宽让给当前曲）
    // low：不打断 high，也不互取消
    if (priority === 'high') {
      if (musicHighController) {
        try {
          musicHighController.abort()
        } catch {
          /* ignore */
        }
      }
      for (const c of musicLowControllers) {
        try {
          c.abort()
        } catch {
          /* ignore */
        }
      }
      musicLowControllers.clear()
    }

    const myController = new AbortController()
    if (priority === 'high') {
      musicHighController = myController
      musicHighUrl = imageUrl
    } else {
      musicLowControllers.add(myController)
    }

    // 小尺寸加速解码；失败则回退原始封面（显示用那张，通常已在缓存）
    const fetchUrl = coverUrlForColorExtract(imageUrl)
    const fallbackUrl = fetchUrl !== imageUrl ? imageUrl : null
    // high：多次；low：2 次（含 URL 回退已在单次 attempt 内完成）
    const maxAttempts =
      priority === 'high' ? MUSIC_HIGH_MAX_ATTEMPTS : 2

    const run = (async (): Promise<ColorPalette> => {
      try {
        let lastError: unknown = null
        for (let attempt = 0; attempt < maxAttempts; attempt++) {
          if (myController.signal.aborted) {
            throw new Error('Extraction cancelled')
          }
          try {
            const palette = await extractFromImage(
              fetchUrl,
              myController.signal,
              fallbackUrl,
            )
            if (myController.signal.aborted) {
              throw new Error('Extraction cancelled')
            }
            // extractFromImage 成功时已保证非 DEFAULT
            setMemoryCache(imageUrl, palette)
            return palette
          } catch (error) {
            if (isAbortError(error)) throw error
            lastError = error
            console.debug(
              '[ColorExtractor] Music extraction attempt failed:',
              attempt + 1,
              error instanceof Error ? error.message : error,
            )
          }
          if (attempt < maxAttempts - 1) {
            await sleepWithSignal(120 * (attempt + 1), myController.signal)
          }
        }
        console.debug(
          '[ColorExtractor] Music extraction exhausted retries:',
          lastError instanceof Error ? lastError.message : lastError,
        )
        return { ...DEFAULT_PALETTE }
      } finally {
        if (priority === 'high' && musicHighController === myController) {
          musicHighController = null
          if (musicHighUrl === imageUrl) musicHighUrl = null
        }
        if (priority === 'low') {
          musicLowControllers.delete(myController)
        }
      }
    })()

    musicInflight.set(imageUrl, run)
    try {
      return await run
    } finally {
      if (musicInflight.get(imageUrl) === run) {
        musicInflight.delete(imageUrl)
      }
    }
  }

  // ── 壁纸 / 通用路径 ──
  if (currentExtractionController) {
    currentExtractionController.abort()
  }

  const myController = new AbortController()
  currentExtractionController = myController
  currentExtractionUrl = imageUrl

  try {
    if (isWallpaper && !wallpaperState.isUrlActive(imageUrl)) {
      console.debug(
        '[ColorExtractor] Wallpaper URL may have changed, but continuing extraction',
      )
    }

    if (!options.forceRefresh) {
      const cached = memoryCache.get(imageUrl) || getLocalStorageCache(imageUrl)
      // 历史上失败的默认灰可能已落盘，视为未命中以便重取
      if (cached && !isDefaultPalette(cached)) {
        setMemoryCache(imageUrl, cached)
        return cached
      }
    }

    const palette = await extractFromImage(imageUrl, myController.signal)

    if (myController.signal.aborted) {
      throw new Error('Extraction cancelled')
    }

    // 与 setMemoryCache 对齐：取色失败的占位灰不落盘，否则会被永久缓存
    setMemoryCache(imageUrl, palette)
    if (!isDefaultPalette(palette)) {
      saveToLocalStorage(imageUrl, palette)
    }

    return palette
  } catch (error) {
    console.debug(
      '[ColorExtractor] Extraction failed:',
      error instanceof Error ? error.message : error,
    )

    if (error instanceof Error) {
      if (
        error.message.includes('cancel') ||
        error.message.includes('Abort') ||
        error.message.includes('Wallpaper')
      ) {
        throw error
      }
    }
    return { ...DEFAULT_PALETTE }
  } finally {
    if (currentExtractionController === myController) {
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
  // 设置强调色：与 Hero adaptive 同源（对比度可读）
  syncCfgAccentColor()
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
  } else {
    memoryCache.clear()
    localStorage.removeItem('wallpaperColorCache')
  }
}

/**
 * 从已加载的 HTMLImageElement 直接提取颜色（零网络，封面 onload 热路径）。
 * 同源 / 已带 CORS 的图可读像素；跨域无 CORS 仍会失败并返回 DEFAULT。
 */
export function extractColorsFromLoadedImage(
  img: HTMLImageElement,
): ColorPalette {
  try {
    const naturalW = img.naturalWidth || img.width
    const naturalH = img.naturalHeight || img.height
    return paletteFromRaster(naturalW, naturalH, (ctx) => {
      ctx.drawImage(img, 0, 0, ctx.canvas.width, ctx.canvas.height)
    })
  } catch (err) {
    console.debug(
      '[ColorExtractor] Cannot extract from loaded image:',
      err instanceof Error ? err.message : err,
    )
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
