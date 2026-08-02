/**
 * 分平台硬件档位判定（动画 standard/light vs light/exlight 的「是否达标」）。
 *
 * OS / iOS 版本解析见 `platformDetect.ts`（与 WebKit、音频通路共用）。
 *
 * 产品规则（2026-07）：
 * - Android：内存 ≥ 8GB 且 CPU 逻辑核 ≥ 8 → 高
 * - iOS：系统主版本 >= 18 → 高；小于 18 → 低（含未来 26/27 等）
 * - macOS：Apple Silicon (M 系列) → 高；Intel → 低
 * - Windows / Linux：内存档 ≥ 12GB 级且逻辑核 ≥ 6 → 高
 *
 * 浏览器限制：
 * - `deviceMemory` 多为 0.25/0.5/1/2/4/8 分桶，≥8 表示「约 8GB 及以上」
 *   Windows「12G+」在 API 上用 ≥8 桶近似（12–16G 机器通常仍报 8）。
 * - Safari 无 deviceMemory；iOS 不以核数/内存分档。
 * - macOS 上 `navigator.platform` 在 M 芯片仍常为 MacIntel，需 architecture / WebGL 辅助。
 */

import type { OsKind } from './platformDetect'
import {
  detectOsKind,

  parseIosMajorVersion,
} from './platformDetect'

export type { OsKind }
export { detectOsKind, parseIosMajorVersion }

export interface HardwareSignals {
  os: OsKind
  /** navigator.hardwareConcurrency */
  cores: number | null
  /** navigator.deviceMemory（GiB 近似分桶，可能为 null） */
  memoryGiB: number | null
  /** iOS 主版本，非 iOS 为 null */
  iosMajor: number | null
  /** macOS 是否判定为 Apple Silicon */
  appleSilicon: boolean | null
}

export interface HardwareTierResult {
  /** 是否达到「高硬件」门槛（可走 standard↔light） */
  highHardware: boolean
  signals: HardwareSignals
  /** 简短原因，便于 debug */
  reason: string
}

// —— Apple Silicon 探测（WebGL 最多一次）————————————————————————

/**
 * 最终结果缓存。
 * - `undefined`：尚未得出结论（Chromium 可能在等 async architecture）
 * - `null`：已尽力探测仍无法判断（不再开 WebGL）
 * - `true` / `false`：已确认
 */
let cachedAppleSilicon: boolean | null | undefined
/** 本页是否已创建过 WebGL 探测上下文 */
let webglProbeDone = false

function loseWebGlContext(gl: WebGLRenderingContext | WebGL2RenderingContext) {
  try {
    const lose = gl.getExtension('WEBGL_lose_context') as {
      loseContext?: () => void
    } | null
    lose?.loseContext?.()
  } catch {
    /* ignore */
  }
}

function hasHighEntropyArchitectureApi(nav: Navigator): boolean {
  const uaData = (
    nav as Navigator & {
      userAgentData?: {
        getHighEntropyValues?: (hints: string[]) => Promise<unknown>
      }
    }
  ).userAgentData
  return typeof uaData?.getHighEntropyValues === 'function'
}

/** 本页最多创建一次 WebGL；结果写入 `cachedAppleSilicon`。 */
function probeWebGlOnce(): boolean | null {
  if (cachedAppleSilicon !== undefined) return cachedAppleSilicon
  if (webglProbeDone) {
    cachedAppleSilicon = null
    return null
  }
  webglProbeDone = true

  try {
    if (
      typeof document === 'undefined' ||
      typeof WebGLRenderingContext === 'undefined'
    ) {
      cachedAppleSilicon = null
      return null
    }
    const canvas = document.createElement('canvas')
    const gl = canvas.getContext('webgl', {
      failIfMajorPerformanceCaveat: false,
      powerPreference: 'low-power',
    }) as WebGLRenderingContext | null
    if (!gl || typeof gl.getExtension !== 'function') {
      cachedAppleSilicon = null
      return null
    }
    try {
      const dbg = gl.getExtension('WEBGL_debug_renderer_info')
      if (dbg) {
        const renderer = gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) as string
        if (typeof renderer === 'string' && renderer) {
          if (/Apple\s+M\d/i.test(renderer) || /Apple GPU/i.test(renderer)) {
            cachedAppleSilicon = true
            return true
          }
          if (/Intel|AMD|NVIDIA/i.test(renderer) && !/Apple/i.test(renderer)) {
            cachedAppleSilicon = false
            return false
          }
          if (/Apple/i.test(renderer) && !/Intel/i.test(renderer)) {
            cachedAppleSilicon = true
            return true
          }
        }
      }
    } finally {
      loseWebGlContext(gl)
      canvas.width = 0
      canvas.height = 0
    }
  } catch {
    /* ignore */
  }

  cachedAppleSilicon = null
  return null
}

/**
 * 同步探测。
 * - 已有结论 → 直接返回
 * - UA 明确 arm64 → true
 * - Chromium（有 high-entropy API）→ **不**开 WebGL，返回 null，等 async
 * - 其余（Safari 等）→ 最多一次 WebGL
 */
export function detectAppleSilicon(
  nav: Navigator | null = typeof navigator !== 'undefined' ? navigator : null,
): boolean | null {
  if (!nav) return null
  if (cachedAppleSilicon !== undefined) return cachedAppleSilicon

  const ua = typeof nav.userAgent === 'string' ? nav.userAgent : ''
  if (/\b(?:Mac OS X|Macintosh).+\bARM64\b/i.test(ua)) {
    cachedAppleSilicon = true
    return true
  }

  // 同步路径不创建 WebGL；architecture 交给 detectAppleSiliconAsync
  if (hasHighEntropyArchitectureApi(nav)) {
    return null
  }

  return probeWebGlOnce()
}

/** 测试用：清空探测状态 */
export function resetAppleSiliconCache(): void {
  cachedAppleSilicon = undefined
  webglProbeDone = false
}

/**
 * 异步补全：Chromium architecture 优先；失败再 **一次** WebGL 兜底。
 */
export async function detectAppleSiliconAsync(
  nav: Navigator | null = typeof navigator !== 'undefined' ? navigator : null,
): Promise<boolean | null> {
  if (!nav) return null
  if (cachedAppleSilicon === true || cachedAppleSilicon === false) {
    return cachedAppleSilicon
  }

  const uaData = (
    nav as Navigator & {
      userAgentData?: {
        getHighEntropyValues?: (hints: string[]) => Promise<{
          architecture?: string
          platform?: string
        }>
      }
    }
  ).userAgentData

  if (uaData?.getHighEntropyValues) {
    try {
      const values = await uaData.getHighEntropyValues([
        'architecture',
        'platform',
      ])
      const arch = (values.architecture || '').toLowerCase()
      const platform = (values.platform || '').toLowerCase()
      if (platform === 'macos' || platform === '') {
        if (arch === 'arm' || arch === 'arm64') {
          cachedAppleSilicon = true
          return true
        }
        if (arch === 'x86' || arch === 'x86_64') {
          cachedAppleSilicon = false
          return false
        }
      }
    } catch {
      /* fall through */
    }
  }

  // architecture 不可用：允许一次 WebGL（含 Chromium 失败路径）
  return probeWebGlOnce()
}

// —— 分平台规则 ————————————————————————————————————————————————

function readCores(
  nav: Navigator | null = typeof navigator !== 'undefined' ? navigator : null,
): number | null {
  if (!nav) return null
  const n = nav.hardwareConcurrency
  return typeof n === 'number' && n > 0 ? n : null
}

function readMemoryGiB(
  nav: Navigator | null = typeof navigator !== 'undefined' ? navigator : null,
): number | null {
  if (!nav) return null
  const mem = (nav as Navigator & { deviceMemory?: number }).deviceMemory
  return typeof mem === 'number' && mem > 0 ? mem : null
}

/**
 * 收集信号（同步）。macOS Apple Silicon 可能为 null，需 async 补全。
 */
export function collectHardwareSignals(
  nav: Navigator | null = typeof navigator !== 'undefined' ? navigator : null,
  ua?: string,
): HardwareSignals {
  const os = detectOsKind(ua, nav)
  return {
    os,
    cores: readCores(nav),
    memoryGiB: readMemoryGiB(nav),
    iosMajor: os === 'ios' ? parseIosMajorVersion(ua) : null,
    appleSilicon: os === 'macos' ? detectAppleSilicon(nav) : null,
  }
}

/**
 * 是否「高硬件」：
 * - Android：≥8GB 档 且 ≥8 核
 * - iOS：系统主版本 ≥ 18
 * - macOS：M 系列；Intel 为低；无法识别时保守为低
 * - Windows/Linux：≥12GB 级（API 用 ≥8 桶）且 ≥6 核
 * - unknown：核与内存同时偏高才给高，否则低
 */
export function evaluateHighHardware(
  signals: HardwareSignals,
): HardwareTierResult {
  const { os, cores, memoryGiB, iosMajor, appleSilicon } = signals

  switch (os) {
    case 'android': {
      const memOk = memoryGiB != null && memoryGiB >= 8
      const cpuOk = cores != null && cores >= 8
      if (memOk && cpuOk) {
        return {
          highHardware: true,
          signals,
          reason: `android high: mem=${memoryGiB} cores=${cores}`,
        }
      }
      return {
        highHardware: false,
        signals,
        reason: `android low: mem=${memoryGiB ?? 'n/a'} cores=${cores ?? 'n/a'} (need ≥8GB & ≥8 cores)`,
      }
    }

    case 'ios': {
      if (iosMajor == null) {
        return {
          highHardware: false,
          signals,
          reason: 'ios low: version unknown',
        }
      }
      if (iosMajor >= 18) {
        return {
          highHardware: true,
          signals,
          reason: `ios high: iOS ${iosMajor}`,
        }
      }
      return {
        highHardware: false,
        signals,
        reason: `ios low: iOS ${iosMajor} < 18`,
      }
    }

    case 'macos': {
      if (appleSilicon === true) {
        return {
          highHardware: true,
          signals,
          reason: 'macos high: Apple Silicon',
        }
      }
      if (appleSilicon === false) {
        return {
          highHardware: false,
          signals,
          reason: 'macos low: Intel',
        }
      }
      return {
        highHardware: false,
        signals,
        reason: 'macos low: chip unknown (conservative)',
      }
    }

    case 'windows':
    case 'linux': {
      const memOk = memoryGiB != null && memoryGiB >= 8
      const cpuOk = cores != null && cores >= 6
      if (memoryGiB == null) {
        if (cores != null && cores >= 8) {
          return {
            highHardware: true,
            signals,
            reason: `${os} high: cores=${cores} (mem n/a, cores≥8)`,
          }
        }
        return {
          highHardware: false,
          signals,
          reason: `${os} low: mem n/a cores=${cores ?? 'n/a'}`,
        }
      }
      if (memOk && cpuOk) {
        return {
          highHardware: true,
          signals,
          reason: `${os} high: memBucket=${memoryGiB} cores=${cores}`,
        }
      }
      return {
        highHardware: false,
        signals,
        reason: `${os} low: memBucket=${memoryGiB} cores=${cores ?? 'n/a'} (need ~12GB+ & ≥6 cores)`,
      }
    }

    default: {
      if (
        memoryGiB != null &&
        memoryGiB >= 8 &&
        cores != null &&
        cores >= 8
      ) {
        return {
          highHardware: true,
          signals,
          reason: 'unknown high: mem≥8 cores≥8',
        }
      }
      return {
        highHardware: false,
        signals,
        reason: 'unknown low: conservative',
      }
    }
  }
}

/** 同步评估当前环境是否高硬件 */
export function evaluateCurrentHardwareTier(): HardwareTierResult {
  return evaluateHighHardware(collectHardwareSignals())
}
