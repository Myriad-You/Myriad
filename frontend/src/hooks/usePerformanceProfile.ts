import type { HardwareSignals, OsKind } from '../utils/deviceHardwareTier'
import { useSyncExternalStore } from 'react'
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

/* ============================================================
   共享 store
   ------------------------------------------------------------
   此前每个调用点各持一份 useState + 一个 prefers-reduced-motion
   MediaQueryList + 一次 syncHardwareToDocument()。首页十几个小组件
   （每个都经 useAnimationLevel 走到这里）会堆出十几个 MQL 监听、
   十几次根元素 dataset 写入（每次让整份文档样式失效）。

   探测结果本来就是模块级单例（cachedProfile），这里只是把订阅一并收敛：
   一次探测、一个监听、一次 DOM 同步、一组订阅者。
   ============================================================ */

const _profileListeners = new Set<() => void>()
let _profileBootstrapped = false

/** 只在真正变化时换引用——getSnapshot 必须返回稳定引用 */
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
  if (prev && sameProfile(prev, next)) {
    // 保住旧引用，订阅者不必重渲染
    cachedProfile = prev
    return
  }
  cachedProfile = next
  syncHardwareToDocument(next)
  for (const listener of _profileListeners) listener()
}

/** 不经缓存地重新读一次硬件与视口标志 */
function readProfileFresh(): PerformanceProfile {
  const { isMobile, reduceMotion } = readViewportFlags()
  return buildProfile(collectHardwareSignals(), reduceMotion, isMobile)
}

/** 全进程一次：写 DOM 标志、挂 reduced-motion 监听、补 macOS 芯片探测 */
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

  // macOS：同步可能认不出芯片（保守 low）。async architecture / 单次 WebGL 后再升/降。
  // 用 highHardware 是否变化判断，勿用 appleSilicon ===（async 已写缓存时恒等）。
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

/** 首帧用同步探测，避免 DEFAULT highHardware:true 闪一下再降档 */
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
