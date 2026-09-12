import { useMemo } from 'react'

import { isExlight, useAnimationLevel } from '../../useAnimationLevel'
import { registerPageCleanup } from '../core'
import { brewMotionQuiet, brewMotionReset } from './brewMotion'
import { resetBrewTagIds } from './brewTag'

export {
  awaitLaneSwap,
  brewSurfaceSwapWait,
  brewTagSwapWait,
  chipEnterFrames,
  chipExitFrames,
  diffChipKeys,
  planChipLaneSwap,
  playBrewChipEnter,
  playBrewChipExit,
  playBrewSurfaceExit,
  shouldPlayChipEnter,
} from './brewChipPresence'
export {
  brewMotionClaim,
  brewMotionLane,
  brewMotionOwns,
  brewMotionQuiet,
  brewMotionRelease,
  brewMotionReset,
} from './brewMotion'
export {
  BREW_TAG_ENTER_MS,
  BREW_TAG_EXIT_MS,
  BREW_TAG_STAGGER_MS,
  brewTagDelay,
  brewTagQuiet,
  useBrewTag,
} from './brewTag'

type AnimationConfig = ReturnType<typeof useAnimationLevel>

const _PAGE_ID = 'brew'
const VEIL_ENTER_DELAY_MS = 48
const VEIL_EXIT_MS = 560

let brewVeilEnterTimer = 0
let brewVeilExitTimer = 0

function brewVeilQuiet(): boolean {
  return brewMotionQuiet()
}

/** 内容就绪后加深底部遮罩（范围 + 浓度）。 */
export function playBrewVeilEnter(): void {
  if (typeof document === 'undefined') return
  window.clearTimeout(brewVeilExitTimer)
  brewVeilExitTimer = 0

  const root = document.documentElement
  const host = document.getElementById('bg-gradient')
  root.setAttribute('data-brew-veil', '')
  if (!host) return

  let bloom = document.getElementById('brew-veil-bloom')
  if (!bloom) {
    bloom = document.createElement('div')
    bloom.id = 'brew-veil-bloom'
    bloom.setAttribute('aria-hidden', 'true')
    host.appendChild(bloom)
    void bloom.offsetWidth
  }

  window.clearTimeout(brewVeilEnterTimer)
  brewVeilEnterTimer = window.setTimeout(() => {
    if (brewVeilQuiet()) bloom.style.opacity = '1'
    else bloom.classList.add('is-on')
    brewVeilEnterTimer = 0
  }, VEIL_ENTER_DELAY_MS)
}

/** 离开时先收浓度，再卸节点，避免硬切。 */
export function playBrewVeilExit(): void {
  if (typeof document === 'undefined') return
  window.clearTimeout(brewVeilEnterTimer)
  window.clearTimeout(brewVeilExitTimer)
  brewVeilEnterTimer = 0

  document.documentElement.removeAttribute('data-brew-veil')
  const bloom = document.getElementById('brew-veil-bloom')
  if (!bloom) return

  bloom.classList.remove('is-on')
  bloom.style.removeProperty('opacity')
  if (brewVeilQuiet()) {
    bloom.remove()
    return
  }
  brewVeilExitTimer = window.setTimeout(() => {
    document.getElementById('brew-veil-bloom')?.remove()
    brewVeilExitTimer = 0
  }, VEIL_EXIT_MS + 40)
}

/** startPage('brew') 由 useRouteScheduler 统一调用。 */
export function useBrewScheduler(): void {}

// 按 level 缓存，避免每次新对象。
const ANIM_CONFIG_CACHE = new Map<
  string,
  ReturnType<typeof useBrewAnimationConfig>
>()

export function useBrewAnimationConfig(): AnimationConfig {
  const baseConfig = useAnimationLevel()

  return useMemo(() => {
    const cacheKey = baseConfig.level
    const cached = ANIM_CONFIG_CACHE.get(cacheKey)
    if (cached?.level === baseConfig.level) {
      return cached
    }

    ANIM_CONFIG_CACHE.set(cacheKey, baseConfig)
    return baseConfig
  }, [baseConfig])
}

export const brewAnimationPresets = {

  readerEnter: {
    initial: { opacity: 0, y: 40 },
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, y: 40 },
  },
} as const

export function getBrewTransition(
  animConfig: AnimationConfig,
  type: 'reader' = 'reader',
): { duration: number; ease: [number, number, number, number] } {
  const durations = {
    reader:
      isExlight(animConfig)
        ? 0
        : animConfig.level === 'light'
          ? 0.2
          : 0.4,
  }

  return {
    duration: durations[type],
    ease: [0.16, 1, 0.3, 1],
  }
}

export function cleanupBrew(): void {
  ANIM_CONFIG_CACHE.clear()
  resetBrewTagIds()
  brewMotionReset()
  playBrewVeilExit()
}

registerPageCleanup(_PAGE_ID, cleanupBrew)
