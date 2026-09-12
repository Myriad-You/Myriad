/** 全局栏 tag 进出场：按 id 差分，整栏替换先退后进。 */
import type { ReactElement, ReactNode } from 'react'
import { Children, isValidElement } from 'react'

import {
  BREW_TAG_ENTER_MS,
  BREW_TAG_EXIT_MS,
  brewTagDelay,
  brewTagQuiet,
} from './brewTag'

export const BREW_TAG_EXIT_TRANSFORM =
  'translate3d(0, var(--sm-shift-sm, 6px), 0) scale(0.96)'

export const BREW_CARD_EXIT_TRANSFORM = 'translate3d(0, var(--sm-shift-sm, 6px), 0)'

/** 卡片退场最多错开前 8 张，避免长轨把换页拖住。 */
export const BREW_SURFACE_CARD_CAP = 8

export const BREW_SURFACE_CARD_SELECTOR = [
  '[data-brew-card]',
  '.brew-salon__card',
  '.brew-stories .brew-story',
  '.brew-empty',
  '.brew-site',
  '.brew-feeds .brew-story',
].join(',')

export const BREW_CHIP_SELECTOR = '[data-brew-chip]'

export function chipExitFrames(
  opacity: string,
  transform: string,
  toTransform = BREW_TAG_EXIT_TRANSFORM,
): Keyframe[] {
  return [
    {
      opacity,
      transform: transform === 'none' ? 'none' : transform,
    },
    {
      opacity: 0,
      transform: toTransform,
    },
  ]
}

export function chipEnterFrames(
  opacity: string,
  transform: string,
): Keyframe[] {
  return [
    {
      opacity: 0,
      transform: BREW_TAG_EXIT_TRANSFORM,
    },
    {
      opacity,
      transform: transform === 'none' ? 'none' : transform,
    },
  ]
}

function motionTargets(root: HTMLElement | null, selector: string): HTMLElement[] {
  if (!root) return []
  return Iterator.from(root.querySelectorAll<HTMLElement>(selector))
    .filter(
      (el) => !el.dataset.brewGhost && !el.classList.contains('is-ghosted'),
    )
    .toArray()
}

function playMotionExit(
  nodes: HTMLElement[],
  toTransform: string,
  cap = Infinity,
): Promise<void>[] {
  const pending: Promise<void>[] = []
  nodes.forEach((el, index) => {
    if (typeof el.animate !== 'function') return
    const style = getComputedStyle(el)
    for (const anim of el.getAnimations()) anim.cancel()
    const anim = el.animate(
      chipExitFrames(style.opacity, style.transform, toTransform),
      {
        duration: BREW_TAG_EXIT_MS,
        delay: brewTagDelay(Math.min(index, Math.max(cap - 1, 0))),
        easing: 'cubic-bezier(0.4, 0, 1, 1)',
        fill: 'forwards',
      },
    )
    pending.push(anim.finished.then(() => undefined, () => undefined))
  })
  return pending
}

function exitResult(
  count: number,
  pending: Promise<void>[],
  quiet: boolean,
): {
  wait: number
  waapi: boolean
  done: Promise<void>
} {
  const wait = brewTagSwapWait(Math.max(count, 1), quiet)
  if (quiet || pending.length === 0) {
    return { wait, waapi: false, done: Promise.resolve() }
  }
  return {
    wait,
    waapi: true,
    done: Promise.all(pending).then(() => undefined),
  }
}

/** 从当前计算值退到 0，避免关键帧 from:1 把半透明弹成全显。 */
export function playBrewChipExit(row: HTMLElement | null): {
  wait: number
  waapi: boolean
  done: Promise<void>
} {
  const chips = motionTargets(row, BREW_CHIP_SELECTOR)
  const quiet = brewTagQuiet()
  if (quiet || chips.length === 0) {
    return exitResult(chips.length, [], quiet)
  }
  return exitResult(
    chips.length,
    playMotionExit(chips, BREW_TAG_EXIT_TRANSFORM),
    quiet,
  )
}

/** 栏 + 卡片一起退，给板块 / 筛选换树用。 */
export function playBrewSurfaceExit(root: HTMLElement | null): {
  wait: number
  waapi: boolean
  done: Promise<void>
} {
  const quiet = brewTagQuiet()
  const chips = motionTargets(root, BREW_CHIP_SELECTOR)
  const cards = motionTargets(root, BREW_SURFACE_CARD_SELECTOR).filter(
    (el) => !el.closest(BREW_CHIP_SELECTOR),
  )
  const wait = brewSurfaceSwapWait(chips.length, cards.length, quiet)
  if (quiet || (chips.length === 0 && cards.length === 0)) {
    return { wait, waapi: false, done: Promise.resolve() }
  }
  const pending = [
    ...playMotionExit(chips, BREW_TAG_EXIT_TRANSFORM),
    ...playMotionExit(cards, BREW_CARD_EXIT_TRANSFORM, BREW_SURFACE_CARD_CAP),
  ]
  return {
    wait,
    waapi: pending.length > 0,
    done: pending.length
      ? Promise.all(pending).then(() => undefined)
      : Promise.resolve(),
  }
}

/** 换树后入场。fill:both 盖住首帧，避免新 tag 先以 opacity:1 闪一帧。 */
export function playBrewChipEnter(row: HTMLElement | null): void {
  if (brewTagQuiet() || !row) return
  motionTargets(row, BREW_CHIP_SELECTOR).forEach((el, index) => {
    if (typeof el.animate !== 'function') return
    if (el.classList.contains('is-conceal')) return
    for (const anim of el.getAnimations()) anim.cancel()
    el.animate(chipEnterFrames('1', 'none'), {
      duration: BREW_TAG_ENTER_MS,
      delay: brewTagDelay(index),
      easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
      fill: 'both',
    })
  })
}

/** 退场未结束不播入场：否则入场在隐藏层播完，揭开时已经停在 opacity:1。 */
export function shouldPlayChipEnter(frozen: boolean): boolean {
  return !frozen
}

export type ChipPhase = 'enter' | 'in' | 'exit'

/** 退场结束后再换树，给最后一枚一点收尾余量。 */
export const BREW_TAG_SWAP_PAD_MS = 32

export type ChipLanePlan = 'hold' | 'start-exit' | 'retarget'

/** 退场进行中只改目的地，不重开节拍。 */
export function planChipLaneSwap(
  displayedWave: string,
  nextWave: string,
  exiting: boolean,
): ChipLanePlan {
  if (exiting) return 'retarget'
  if (nextWave === displayedWave) return 'hold'
  return 'start-exit'
}

export function diffChipKeys(
  prev: readonly string[],
  next: readonly string[],
): {
  leave: string[]
  stay: string[]
  enter: string[]
  fullSwap: boolean
} {
  const prevSet = new Set(prev)
  const nextSet = new Set(next)
  // difference 固定遍历接收者；intersection 会改走较小集合，不能保 prev 序。
  const leave = Iterator.from(prevSet.difference(nextSet)).toArray()
  const stay = Iterator.from(prev)
    .filter((key) => nextSet.has(key))
    .toArray()
  const enter = Iterator.from(nextSet.difference(prevSet)).toArray()
  return {
    leave,
    stay,
    enter,
    fullSwap: leave.length > 0 && stay.length === 0,
  }
}

export function brewTagSwapWait(count: number, quiet = false): number {
  if (quiet || count <= 0) return 0
  return brewTagDelay(count - 1) + BREW_TAG_EXIT_MS + BREW_TAG_SWAP_PAD_MS
}

export function brewSurfaceSwapWait(
  chipCount: number,
  cardCount: number,
  quiet = false,
): number {
  return Math.max(
    brewTagSwapWait(chipCount, quiet),
    brewTagSwapWait(Math.min(cardCount, BREW_SURFACE_CARD_CAP), quiet),
  )
}

/** 按退场时长满拍再换树。不听 WAAPI finished：加速时间轴会立刻 finished。 */
export function awaitLaneSwap(
  _done: Promise<void>,
  wait: number,
): Promise<void> {
  if (wait <= 0) return Promise.resolve()
  return new Promise((resolve) => {
    setTimeout(resolve, wait)
  })
}

export function collectChipNodes(
  children: ReactNode,
): ReactElement<{ id: string }>[] {
  const out: ReactElement<{ id: string }>[] = []
  const walk = (node: ReactNode) => {
    Children.forEach(node, (child) => {
      if (!isValidElement(child)) return
      const id = (child.props as { id?: string }).id
      if (typeof id === 'string' && id) {
        out.push(child as ReactElement<{ id: string }>)
        return
      }
      const nested = (child.props as { children?: ReactNode }).children
      if (nested != null) walk(nested)
    })
  }
  walk(children)
  return out
}
