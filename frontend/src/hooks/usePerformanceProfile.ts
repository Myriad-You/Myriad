import type { HardwareSignals, OsKind } from '../utils/deviceHardwareTier'
import { useSyncExternalStore } from 'react'
import {
  collectHardwareSignals,
  detectAppleSiliconAsync,
  evaluateHighHardware,

} from '../utils/deviceHardwareTier'

/** 硬件是否达标只看 highHardware；动效降级看 data-perf-mode / useAnimationLevel。 */
export interface PerformanceProfile {
  isMobile: boolean
  reduceMotion: boolean

  highHardware: boolean
  os: OsKind
  hardwareConcurrency: number | null
  deviceMemory: number | null

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

/** 生产仅 OS/高低；reason 仅 DEV。 */
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

const _profileListeners = new Set<() => void>()
let _profileBootstrapped = false

/** getSnapshot 必须返回稳定引用。 */
function sameProfile(a: PerformanceProfile, b: PerformanceProfile): boolean {
  return (
    a.isMobile === b.isMobile &&
    a.reduceMotion === b.reduceMotion &&
    a.highHardware === b.highHardware &&
    a.os === b.os &&
    a.hardwareConcurrency === b.hardwareConcurrency &&
    a.deviceMemory === b.deviceMemory
  )
}

function publishProfile(next: PerformanceProfile): void {
  const prev = cachedProfile
  hasDetected = true
  // 保住旧引用，订阅者不必重渲染。
  if (prev && sameProfile(prev, next)) {
    cachedProfile = prev
    return
  }
  cachedProfile = next
  syncHardwareToDocument(next)
  for (const listener of _profileListeners) listener()
}

function readProfileFresh(): PerformanceProfile {
  const { isMobile, reduceMotion } = readViewportFlags()
  return buildProfile(collectHardwareSignals(), reduceMotion, isMobile)
}

function bootstrapProfileStore(): void {
  if (_profileBootstrapped || typeof window === 'undefined') return
  _profileBootstrapped = true

  const detected = detectPerformanceProfile()
  syncHardwareToDocument(detected)

  window
    .matchMedia('(prefers-reduced-motion: reduce)')
    .addEventListener('change', () => {
      publishProfile(readProfileFresh())
    })

  // macOS 同步可能认不出芯片。用 highHardware 是否变化判断，勿用 appleSilicon ===。
  if (detected.os === 'macos') {
    void detectAppleSiliconAsync().then((appleSilicon) => {
      if (appleSilicon == null) return
      const signals = collectHardwareSignals()
      signals.appleSilicon = appleSilicon
      const { isMobile, reduceMotion } = readViewportFlags()
      publishProfile(buildProfile(signals, reduceMotion, isMobile))
    })
  }
}

function subscribeProfile(onStoreChange: () => void): () => void {
  bootstrapProfileStore()
  _profileListeners.add(onStoreChange)
  return () => {
    _profileListeners.delete(onStoreChange)
  }
}

/** 首帧用同步探测，避免 DEFAULT highHardware:true 闪一下再降档。 */
function profileSnapshot(): PerformanceProfile {
  if (typeof window === 'undefined') return DEFAULT_PROFILE
  try {
    return detectPerformanceProfile()
  } catch {
    return DEFAULT_PROFILE
  }
}

function profileServerSnapshot(): PerformanceProfile {
  return DEFAULT_PROFILE
}

export function usePerformanceProfile(): PerformanceProfile {
  return useSyncExternalStore(
    subscribeProfile,
    profileSnapshot,
    profileServerSnapshot,
  )
}
