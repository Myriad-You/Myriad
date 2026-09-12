import type { OsKind } from './platformDetect'
import {
  detectOsKind,

  parseIosMajorVersion,
} from './platformDetect'

export type { OsKind }
export { detectOsKind, parseIosMajorVersion }

export interface HardwareSignals {
  os: OsKind
  cores: number | null
  /** GiB; may be null. */
  memoryGiB: number | null
  iosMajor: number | null
  appleSilicon: boolean | null
}

export interface HardwareTierResult {
  highHardware: boolean
  signals: HardwareSignals
  reason: string
}

let cachedAppleSilicon: boolean | null | undefined
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

/** At most one WebGL probe per page. */
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

  // Sync path must not create WebGL.
  if (hasHighEntropyArchitectureApi(nav)) {
    return null
  }

  return probeWebGlOnce()
}

export function resetAppleSiliconCache(): void {
  cachedAppleSilicon = undefined
  webglProbeDone = false
}

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

  // If architecture is missing, allow one WebGL probe.
  return probeWebGlOnce()
}

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
