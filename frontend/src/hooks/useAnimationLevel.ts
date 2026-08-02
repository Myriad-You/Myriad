import type { PerformanceProfile } from './usePerformanceProfile'
import { useContext, useEffect, useMemo, useState } from 'react'
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

/**
 * Effective animation / visual-effects tier after hardware mapping.
 *
 * User still toggles two preferences only (`light` | `standard` in storage):
 * - Meets hardware bar:  low → `light`,  high → `standard`
 * - Below hardware bar:  low → `exlight`, high → `light`
 *
 * `prefers-reduced-motion` also resolves to `exlight`.
 */
export type AnimationLevel = 'exlight' | 'light' | 'standard'

/** User-facing two-way preference stored in localStorage (plus `auto`). */
export type AnimationUserPreference = 'auto' | 'standard' | 'light'

export interface AnimationConfig {
  level: AnimationLevel
  /** Allow infinite loops (CSS/JS). */
  loop: boolean
  /** Allow spring physics. */
  spring: boolean
  /** Multiply base duration. */
  durationScale: number
  /** Widget GlowBackground (static or animated). */
  widgetGlow: boolean
  /** Widget UI carousels / overview↔detail auto flip. */
  widgetUiRotation: boolean
}

/**
 * Minimal tier — weak-hardware "low" slot AND prefers-reduced-motion.
 * Global backdrop kill is CSS-driven via `html[data-perf-mode=exlight]`.
 */
const CONFIG_EXLIGHT: AnimationConfig = {
  level: 'exlight',
  loop: false,
  spring: false,
  durationScale: 0.4,
  widgetGlow: false,
  widgetUiRotation: false,
}

/**
 * Mid / capable-hardware "low" / weak-hardware "high".
 * Same visual budget as the former `light` tier.
 */
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

/** Anything with a `.level` field (AnimationConfig, sandbox ref, etc.). */
type LevelLike = AnimationLevel | { level: AnimationLevel } | null | undefined

function asLevel(input?: LevelLike): AnimationLevel | undefined {
  if (input == null) return undefined
  if (typeof input === 'string') return input
  return input.level
}

/** Full-effects tier (standard only). */
export function isStandardAnimation(input?: LevelLike): boolean {
  return asLevel(input ?? currentAnimationConfig) === 'standard'
}

/** Minimal tier (exlight). */
export function isExlight(input?: LevelLike): boolean {
  return asLevel(input ?? currentAnimationConfig) === 'exlight'
}

/** Any non-standard tier (light | exlight) — prefer this over duplicating level checks. */
export function isReducedAnimation(input?: LevelLike): boolean {
  const level = asLevel(input ?? currentAnimationConfig)
  return level === 'exlight' || level === 'light'
}

/**
 * Whether the device may run the full `standard` tier as its "high" slot.
 * Rules: `utils/deviceHardwareTier.ts` → `perf.highHardware`.
 */
export function meetsAnimationHardwareRequirement(
  perf: PerformanceProfile,
): boolean {
  return perf.highHardware === true
}

/**
 * Map two-way user preference → effective config under current hardware.
 *
 * - prefers-reduced-motion → always `exlight` (not overridable)
 * - `wantHigh === true`  →  capable: standard · weak: light
 * - `wantHigh === false` →  capable: light    · weak: exlight
 * - `auto`：默认高档；采样仅在帧质**明显很差**时降为低档，记 localStorage（可降不可升）
 * - 手动 standard / light：不走采样
 */
export function resolveAnimationConfig(
  userPref: AnimationUserPreference | null | undefined,
  perf: PerformanceProfile,
  /** auto 是否选高档（localStorage）；仅 userPref 为 auto 时生效 */
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
    // auto / unset
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

/**
 * 同步获取动画配置（用于模块初始化时）
 * 注意：首次渲染前可能尚未读到用户手动偏好；偏好生效后请用 getCurrentAnimationConfig
 */
export function getAnimationConfigSync(): AnimationConfig {
  const perf = getPerformanceProfileSync()
  const pref = readStoredUserPreference() ?? 'auto'
  const autoWantHigh =
    pref === 'auto' || pref == null ? getStoredAutoWantHigh() : true
  return resolveAnimationConfig(pref, perf, autoWantHigh)
}

/** 模块级缓存：供非 React 回调（如音乐呼吸动画）读取用户偏好后的真实级别 */
let currentAnimationConfig: AnimationConfig = getAnimationConfigSync()

/**
 * 获取当前生效的动画配置（含用户手动「低/高」偏好 + 硬件映射）
 * 由 useAnimationLevel 挂载后持续更新
 */
export function getCurrentAnimationConfig(): AnimationConfig {
  return currentAnimationConfig
}

function syncPerfModeToDocument(level: AnimationLevel): void {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.perfMode = level
}

/*
 * 模块加载时立刻写 data-perf-mode（仅客户端）。
 * 避免首屏在 useEffect 前仍按 standard 画毛玻璃；
 * 解析结果与 resolveAnimationConfig 一致：正常硬件默认仍是 standard/light，
 * 不会把正常设备误标成 exlight。
 */
if (typeof document !== 'undefined') {
  try {
    syncPerfModeToDocument(currentAnimationConfig.level)
  } catch {
    /* ignore */
  }
}

export function useAnimationLevel(): AnimationConfig {
  const perf = usePerformanceProfile()
  const prefContext = useContext(AnimationPreferenceContext)
  const pref = (prefContext?.preference ??
    readStoredUserPreference() ??
    'auto') as AnimationUserPreference

  // localStorage 为 auto 高/低真源；epoch 在 demote 后 +1 触发重读
  const [autoEpoch, setAutoEpoch] = useState(0)
  const autoWantHigh = useMemo(() => {
    if (pref !== 'auto' && pref != null) return true
    return getStoredAutoWantHigh()
  }, [pref, autoEpoch])

  const config = useMemo(
    () => resolveAnimationConfig(pref, perf, autoWantHigh),
    [perf, pref, autoWantHigh],
  )

  // 仅 auto：空闲后全局只采一次；手动档不跑
  useEffect(() => {
    if (pref !== 'auto') return
    if (perf.reduceMotion) return
    if (!autoWantHigh) return

    return startAutoFrameAdapt({
      enabled: true,
      onDemote: () => {
        setAutoEpoch((n) => n + 1)
      },
    })
  }, [pref, perf.reduceMotion, autoWantHigh])

  useEffect(() => {
    currentAnimationConfig = config
    syncPerfModeToDocument(config.level)
  }, [config])

  useEffect(() => {
    const isMobile = perf.isMobile

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
  }, [config.level, perf.isMobile])

  return config
}
