import {
  harmonizeGradientPalette,
  pickGradientCompanion,
} from './colorHarmony'
import {
  DEFAULT_PALETTE,
  isDefaultPalette,
  type ColorPalette,
} from './colorPalette'
import { coverUrlForColorExtract } from './coverUrlForColorExtract'
import { imagePool, withPooledCanvas } from './objectPool'
import { wallpaperState } from './wallpaperState'
import { yieldToMain } from './yieldToMain'

export { coverUrlForColorExtract } from './coverUrlForColorExtract'
export {
  applyColorPalette,
  DEFAULT_PALETTE,
  isDefaultPalette,
  type ColorPalette,
} from './colorPalette'

interface CachedColorData {
  url: string
  palette: ColorPalette
  timestamp: number
  version: number
}

interface ExtractOptions {
  forceRefresh?: boolean
  context?: 'wallpaper' | 'music' | string
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

const CACHE_VERSION = 6
const CACHE_EXPIRY_MS = 30 * 24 * 60 * 60 * 1000 // 30d localStorage
const MAX_CANVAS_SIZE = 150
const SAMPLE_STEP = 4
const COLOR_QUANTIZE_STEP = 16

const COLOR_THRESHOLDS = {
  minSaturation: 0.35,
  minChroma: 50,
  minGrayDistance: 40,
  minBrightness: 30,
  maxBrightness: 225,
  minPercentage: 2,
  fallbackMinPercentage: 1,
} as const

const MUSIC_HIGH_MAX_ATTEMPTS = 3

const memoryCache = new Map<string, ColorPalette>()

/** LRU cap. */
const MAX_MEMORY_CACHE = 50

/** Do not cache placeholder grey. */
function setMemoryCache(url: string, palette: ColorPalette): void {
  if (isDefaultPalette(palette)) return
  memoryCache.delete(url)
  memoryCache.set(url, palette)
  while (memoryCache.size > MAX_MEMORY_CACHE) {
    const oldest = memoryCache.keys().next().value
    if (oldest === undefined) break
    memoryCache.delete(oldest)
  }
}

let currentExtractionController: AbortController | null = null
let musicHighController: AbortController | null = null
let musicHighUrl: string | null = null
const musicLowControllers = new Set<AbortController>()
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

/** Placeholder grey is a miss. */
export function getCachedPalette(url: string | null | undefined): ColorPalette | null {
  if (!url) return null
  const hit = memoryCache.get(url)
  if (!hit) return null
  if (isDefaultPalette(hit)) {
    memoryCache.delete(url)
    return null
  }
  setMemoryCache(url, hit)
  return hit
}

export function setCachedPalette(
  url: string | null | undefined,
  palette: ColorPalette,
): void {
  if (!url || !palette || isDefaultPalette(palette)) return
  setMemoryCache(url, palette)
}

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
  }
}

function getPerceptualBrightness(r: number, g: number, b: number): number {
  return 0.299 * r + 0.587 * g + 0.114 * b
}

function getSaturation(r: number, g: number, b: number): number {
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  return max === 0 ? 0 : (max - min) / max
}

function getChroma(r: number, g: number, b: number): number {
  return Math.max(r, g, b) - Math.min(r, g, b)
}

function getDistanceFromGray(r: number, g: number, b: number): number {
  const avg = (r + g + b) / 3
  return Math.sqrt((r - avg) ** 2 + (g - avg) ** 2 + (b - avg) ** 2)
}

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

  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const mid = r + g + b - max - min
  if (max - mid < 20 && mid - min < 20) return false

  return true
}

function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  const toHex = (v: number) => clamp(v).toString(16).padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

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

function lightenColor(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b)
  hsl.l = Math.min(0.85, hsl.l + 0.2)
  hsl.s = Math.min(1, hsl.s * 1.1)
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return rgbToHex(rgb.r, rgb.g, rgb.b)
}

function darkenColor(r: number, g: number, b: number): string {
  const hsl = rgbToHsl(r, g, b)
  hsl.l = Math.max(0.15, hsl.l - 0.25)
  hsl.s = Math.min(1, hsl.s * 1.15)
  const rgb = hslToRgb(hsl.h, hsl.s, hsl.l)
  return rgbToHex(rgb.r, rgb.g, rgb.b)
}

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

  const sortedColors: ColorInfo[] = Iterator.from(colorMap.entries())
    .toArray()
    .toSorted((a, b) => b[1] - a[1])
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

/** 1×1/empty images must not cache as success. */
function isDegenerateImageSize(width: number, height: number): boolean {
  return (
    !Number.isFinite(width) ||
    !Number.isFinite(height) ||
    width <= 2 ||
    height <= 2
  )
}

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

/** Pool reset must change src or same-URL skips onload. */
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

      // Must change src so a cached URL still fires load.
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

    // 离开 onload 任务再采样，避免和 decode 挤成一条 Long Task。
    await yieldToMain()
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

export async function extractColorsFromImage(
  imageUrl: string,
  options: ExtractOptions = {},
): Promise<ColorPalette> {
  const isMusic = options.context === 'music'
  const isWallpaper = options.context === 'wallpaper'
  const priority: 'high' | 'low' = options.priority ?? 'high'

  if (isMusic) {
    if (!options.forceRefresh) {
      const cached = getCachedPalette(imageUrl)
      if (cached) return cached
    }

    // Dedupe in-flight by cover URL.
    if (!options.forceRefresh) {
      const inflight = musicInflight.get(imageUrl)
      if (inflight) {
        if (priority === 'high') {
          // Cancel previous high; do not abort this URL's low.
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
          // high must retry DEFAULT; low returns it.
          if (priority === 'low') return reused
        } catch (error) {
          if (isAbortError(error)) throw error
          if (priority === 'low') return { ...DEFAULT_PALETTE }
          console.debug(
            '[ColorExtractor] Music inflight failed, high will retry:',
            error instanceof Error ? error.message : error,
          )
        }
        // high + DEFAULT: do not join the old promise.
      }
    }

    // low must not cancel high or other lows.
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

    const fetchUrl = coverUrlForColorExtract(imageUrl)
    const fallbackUrl = fetchUrl !== imageUrl ? imageUrl : null
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

  if (currentExtractionController) {
    currentExtractionController.abort()
  }

  const myController = new AbortController()
  currentExtractionController = myController

  try {
    if (isWallpaper && !wallpaperState.isUrlActive(imageUrl)) {
      console.debug(
        '[ColorExtractor] Wallpaper URL may have changed, but continuing extraction',
      )
    }

    if (!options.forceRefresh) {
      const cached = memoryCache.get(imageUrl) ?? getLocalStorageCache(imageUrl)
      // Persisted placeholder grey is a miss.
      if (cached && !isDefaultPalette(cached)) {
        setMemoryCache(imageUrl, cached)
        return cached
      }
    }

    const palette = await extractFromImage(imageUrl, myController.signal)

    if (myController.signal.aborted) {
      throw new Error('Extraction cancelled')
    }

    // Do not persist placeholder grey.
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
    }
  }
}

export function clearColorCache(url?: string): void {
  if (url) {
    memoryCache.delete(url)
  } else {
    memoryCache.clear()
    localStorage.removeItem('wallpaperColorCache')
  }
}

/** Needs CORS on the image; else DEFAULT. */
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
