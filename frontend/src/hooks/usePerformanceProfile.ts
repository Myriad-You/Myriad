import type { HardwareSignals, OsKind } from '../utils/deviceHardwareTier'
import { useEffect, useRef, useState } from 'react'
import {
  collectHardwareSignals,
  detectAppleSiliconAsync,
  evaluateHighHardware,

} from '../utils/deviceHardwareTier'

/**
 * 设备性能画像
 *
 * 硬件是否达标只看 `highHardware`（分平台规则见 deviceHardwareTier）。
 * 动效降级看 `data-perf-mode` / useAnimationLevel（exlight|light|standard）。
 */
export interface PerformanceProfile {
  isMobile: boolean
  reduceMotion: boolean
  /** 是否达到分平台高硬件标准（与 reduceMotion 无关） */
  highHardware: boolean
  os: OsKind
  hardwareConcurrency: number | null
  deviceMemory: number | null
  /** 判定原因；仅 dev 写入 DOM */
  hardwareReason?: string
}

const DEFAULT_PROFILE: PerformanceProfile = {
  isMobile: false,
  reduceMotion: false,
  highHardware: true,
  os: 'unknown',
  hardwareConcurrency: null,
  deviceMemory: null,
}

let cachedProfile: PerformanceProfile | null = null
let hasDetected = false

function buildProfile(
  signals: HardwareSignals,
  reduceMotion: boolean,
  isMobile: boolean,
): PerformanceProfile {
  const tier = evaluateHighHardware(signals)
  return {
    isMobile,
    reduceMotion,
    highHardware: tier.highHardware,
    os: signals.os,
    hardwareConcurrency: signals.cores,
    deviceMemory: signals.memoryGiB,
    hardwareReason: tier.reason,
  }
}

function detectPerformanceProfile(): PerformanceProfile {
  if (hasDetected && cachedProfile) {
    return cachedProfile
  }

  if (
    typeof window === 'undefined' ||
    typeof window.matchMedia !== 'function'
  ) {
    return DEFAULT_PROFILE
  }

  try {
    const isMobile = window.matchMedia(
      '(hover: none) and (pointer: coarse)',
    ).matches
    const reduceMotion = window.matchMedia(
      '(prefers-reduced-motion: reduce)',
    ).matches
    const signals = collectHardwareSignals()
    const profile = buildProfile(signals, reduceMotion, isMobile)

    cachedProfile = profile
    hasDetected = true
    return profile
  } catch (e) {
    console.warn('Failed to detect performance profile:', e)
    return DEFAULT_PROFILE
  }
}

export function getPerformanceProfileSync(): PerformanceProfile {
  return detectPerformanceProfile()
}

export function resetPerformanceProfileCache(): void {
  hasDetected = false
  cachedProfile = null
}

/** Sync hardware flags to <html>（生产仅 OS/高低；reason 仅 DEV） */
function syncHardwareToDocument(profile: PerformanceProfile) {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.dataset.deviceOs = profile.os
  root.dataset.highHardware = profile.highHardware ? 'true' : 'false'

  const isDev =
    typeof import.meta !== 'undefined' &&
    Boolean((import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV)
  if (isDev && profile.hardwareReason) {
    root.dataset.hardwareReason = profile.hardwareReason
  } else {
    delete root.dataset.hardwareReason
  }
}

function readViewportFlags() {
  const isMobile = window.matchMedia(
    '(hover: none) and (pointer: coarse)',
  ).matches
  const reduceMotion = window.matchMedia(
    '(prefers-reduced-motion: reduce)',
  ).matches
  return { isMobile, reduceMotion }
}

export function usePerformanceProfile(): PerformanceProfile {
  // 首帧用同步探测，避免 DEFAULT highHardware:true 闪一下再降档
  const [profile, setProfile] = useState<PerformanceProfile>(() => {
    if (typeof window === 'undefined') return DEFAULT_PROFILE
    try {
      return detectPerformanceProfile()
    } catch {
      return DEFAULT_PROFILE
    }
  })
  const hasInitialized = useRef(false)

  useEffect(() => {
    if (hasInitialized.current) return
    hasInitialized.current = true

    let cancelled = false
    const apply = (next: PerformanceProfile) => {
      if (cancelled) return
      resetPerformanceProfileCache()
      cachedProfile = next
      hasDetected = true
      syncHardwareToDocument(next)
      setProfile(next)
    }

    const detected = detectPerformanceProfile()
    syncHardwareToDocument(detected)
    setProfile(detected)

    // macOS：同步可能认不出芯片（保守 low）。async architecture / 单次 WebGL 后再升/降。
    // 用 highHardware 是否变化判断，勿用 appleSilicon ===（async 已写缓存时恒等）。
    if (detected.os === 'macos') {
      void detectAppleSiliconAsync().then((appleSilicon) => {
        if (cancelled || appleSilicon == null) return
        const signals = collectHardwareSignals()
        signals.appleSilicon = appleSilicon
        const { isMobile, reduceMotion } = readViewportFlags()
        const next = buildProfile(signals, reduceMotion, isMobile)
        if (
          cachedProfile &&
          cachedProfile.highHardware === next.highHardware &&
          cachedProfile.os === next.os
        ) {
          return
        }
        apply(next)
      })
    }

    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return

    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)')
    const handler = () => {
      resetPerformanceProfileCache()
      const next = detectPerformanceProfile()
      syncHardwareToDocument(next)
      setProfile(next)
    }

    mediaQuery.addEventListener('change', handler)
    return () => mediaQuery.removeEventListener('change', handler)
  }, [])

  return profile
}
