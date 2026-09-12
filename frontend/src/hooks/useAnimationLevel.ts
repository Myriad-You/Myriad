import type { PerformanceProfile } from './usePerformanceProfile'
import {
  useContext,
  useEffect,
  useMemo,
  useSyncExternalStore,
} from 'react'
import { AnimationPreferenceContext } from '../contexts/AnimationPreferenceContext'
import {
  getStoredAutoWantHigh,
  startAutoFrameAdapt,
} from '../utils/animationAutoAdapt'
import { configureAnimationCoordinator } from './animation'
import {
  getPerformanceProfileSync,

  usePerformanceProfile,
} from './usePerformanceProfile'

/** 硬件映射后的生效档；用户存储仍只有 light | standard（外加 auto）。 */
export type AnimationLevel = 'exlight' | 'light' | 'standard'

export type AnimationUserPreference = 'auto' | 'standard' | 'light'

export interface AnimationConfig {
  level: AnimationLevel

  loop: boolean

  spring: boolean

  durationScale: number

  widgetGlow: boolean

  widgetUiRotation: boolean
}

/** 弱硬件 low 档与 prefers-reduced-motion。毛玻璃由 html[data-perf-mode=exlight] 关。 */
const CONFIG_EXLIGHT: AnimationConfig = {
  level: 'exlight',
  loop: false,
  spring: false,
  durationScale: 0.4,
  widgetGlow: false,
  widgetUiRotation: false,
}

const CONFIG_LIGHT: AnimationConfig = {
  level: 'light',
  loop: false,
  spring: false,
  durationScale: 0.6,
  widgetGlow: true,
  widgetUiRotation: true,
}

const CONFIG_STANDARD: AnimationConfig = {
  level: 'standard',
  loop: true,
  spring: true,
  durationScale: 1.0,
  widgetGlow: true,
  widgetUiRotation: true,
}

type LevelLike = AnimationLevel | { level: AnimationLevel } | null | undefined

function asLevel(input?: LevelLike): AnimationLevel | undefined {
  if (input == null) return undefined
  if (typeof input === 'string') return input
  return input.level
}

export function isStandardAnimation(input?: LevelLike): boolean {
  return asLevel(input ?? currentAnimationConfig) === 'standard'
}

export function isExlight(input?: LevelLike): boolean {
  return asLevel(input ?? currentAnimationConfig) === 'exlight'
}

export function isReducedAnimation(input?: LevelLike): boolean {
  const level = asLevel(input ?? currentAnimationConfig)
  return level === 'exlight' || level === 'light'
}

export function meetsAnimationHardwareRequirement(
  perf: PerformanceProfile,
): boolean {
  return perf.highHardware === true
}

/** reduced-motion 一律 exlight，不可覆盖。auto 默认真高档，采样只降不升。 */
export function resolveAnimationConfig(
  userPref: AnimationUserPreference | null | undefined,
  perf: PerformanceProfile,

  /** auto 是否选高档（localStorage）；仅 userPref 为 auto 时生效。 */
  autoWantHigh: boolean = true,
): AnimationConfig {
  if (perf.reduceMotion) {
    return CONFIG_EXLIGHT
  }

  const capable = meetsAnimationHardwareRequirement(perf)
  let wantHigh: boolean
  if (userPref === 'light') {
    wantHigh = false
  } else if (userPref === 'standard') {
    wantHigh = true
  } else {
    wantHigh = autoWantHigh
  }

  if (capable) {
    return wantHigh ? CONFIG_STANDARD : CONFIG_LIGHT
  }
  return wantHigh ? CONFIG_LIGHT : CONFIG_EXLIGHT
}

function readStoredUserPreference(): AnimationUserPreference | null {
  if (typeof window === 'undefined') return null
  try {
    const stored = localStorage.getItem('animation-preference')
    if (stored === 'auto' || stored === 'standard' || stored === 'light') {
      return stored
    }
  } catch {
    /* ignore */
  }
  return null
}

/** 模块初始化用；偏好生效后改读 getCurrentAnimationConfig。 */
export function getAnimationConfigSync(): AnimationConfig {
  const perf = getPerformanceProfileSync()
  const pref = readStoredUserPreference() ?? 'auto'
  const autoWantHigh =
    pref === 'auto' || pref == null ? getStoredAutoWantHigh() : true
  return resolveAnimationConfig(pref, perf, autoWantHigh)
}

/** 供非 React 回调读取用户偏好后的真实级别。 */
let currentAnimationConfig: AnimationConfig = getAnimationConfigSync()

export function getCurrentAnimationConfig(): AnimationConfig {
  return currentAnimationConfig
}

function syncPerfModeToDocument(level: AnimationLevel): void {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.perfMode = level
}

// 模块加载立刻写 data-perf-mode，避免 useEffect 前按 standard 画毛玻璃。
if (typeof document !== 'undefined') {
  try {
    syncPerfModeToDocument(currentAnimationConfig.level)
  } catch {
    /* ignore */
  }
}

/** demote 后 +1；订阅者据此重读 localStorage 的 auto 高低档。 */
let _autoEpoch = 0
const _autoEpochListeners = new Set<() => void>()
let _autoAdaptStarted = false

function getAutoEpochSnapshot(): number {
  return _autoEpoch
}

function subscribeAutoEpoch(onStoreChange: () => void): () => void {
  _autoEpochListeners.add(onStoreChange)

  // 全局只注册一份 demote 监听（探测本身另有 probeStarted 闸）。
  if (!_autoAdaptStarted) {
    _autoAdaptStarted = true
    startAutoFrameAdapt({
      enabled: true,
      onDemote: () => {
        _autoEpoch += 1
        for (const listener of _autoEpochListeners) listener()
      },
    })
  }

  return () => {
    _autoEpochListeners.delete(onStoreChange)
  }
}

// 已应用到全局的档位/形态，避免同值重复写 DOM 与重配协调器。
let _appliedCoordinatorLevel: AnimationLevel | null = null
let _appliedCoordinatorIsMobile: boolean | null = null

function applyAnimationConfigGlobals(
  config: AnimationConfig,
  isMobile: boolean,
): void {
  if (currentAnimationConfig !== config) {
    currentAnimationConfig = config
    syncPerfModeToDocument(config.level)
  }

  if (
    _appliedCoordinatorLevel === config.level &&
    _appliedCoordinatorIsMobile === isMobile
  ) {
    return
  }
  _appliedCoordinatorLevel = config.level
  _appliedCoordinatorIsMobile = isMobile

  switch (config.level) {
    case 'exlight':
      configureAnimationCoordinator({
        baseConcurrent: 4,
        burstConcurrent: 8,
        burstDuration: 3000,
        maxLoopSlots: 2,
      })
      break
    case 'light':
      configureAnimationCoordinator({
        baseConcurrent: isMobile ? 6 : 10,
        burstConcurrent: isMobile ? 16 : 24,
        burstDuration: 6000,
        maxLoopSlots: isMobile ? 4 : 6,
      })
      break
    case 'standard':
      configureAnimationCoordinator({
        baseConcurrent: isMobile ? 12 : 20,
        burstConcurrent: isMobile ? 32 : 64,
        burstDuration: 10000,
        maxLoopSlots: isMobile ? 8 : 16,
      })
      break
  }
}

export function useAnimationLevel(): AnimationConfig {
  const perf = usePerformanceProfile()
  const prefContext = useContext(AnimationPreferenceContext)
  const pref = (prefContext?.preference ??
    readStoredUserPreference() ??
    'auto') as AnimationUserPreference

  // localStorage 为 auto 高/低真源；手动档不订阅，也就不会启动采样。
  const isAuto = pref === 'auto' && !perf.reduceMotion
  const autoEpoch = useSyncExternalStore(
    isAuto ? subscribeAutoEpoch : noopSubscribe,
    getAutoEpochSnapshot,
    getAutoEpochSnapshot,
  )

  const autoWantHigh = useMemo(() => {
    // autoEpoch 只用来触发重读 localStorage，不参与计算。
    void autoEpoch
    if (pref !== 'auto' && pref != null) return true
    return getStoredAutoWantHigh()
  }, [pref, autoEpoch])

  const config = useMemo(
    () => resolveAnimationConfig(pref, perf, autoWantHigh),
    [perf, pref, autoWantHigh],
  )

  useEffect(() => {
    applyAnimationConfigGlobals(config, perf.isMobile)
  }, [config, perf.isMobile])

  return config
}

function noopSubscribe(): () => void {
  return () => {}
}
