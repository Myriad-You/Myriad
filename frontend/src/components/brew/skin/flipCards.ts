/** 展开文章先退；收回网站卡先落到轨道。 */

import { brewMotionQuiet } from '../../../hooks/animation/pages/brewMotion'
import { RAIL_OVERFLOW_LEFT_PX, railSeatScroll } from './railPan'

export const FLIP_EASE = 'cubic-bezier(0.4, 0.0, 0.2, 1)'
export const FLIP_SITE_MS = 560
export const FLIP_STORY_MS = 400
export const FLIP_STAGGER_MS = 24
export const FLIP_STAGGER_CAP_MS = 192
/** 展开：文章先动，中间不空一拍。 */
export const FLIP_STORY_LEAD_MS = 96
/** 收回：网站卡先飞，不空出文章区。 */
export const FLIP_STORY_FOLLOW_MS = 64
/** 首次入场：网站卡已经在走，文章晚半拍跟上。 */
export const FLIP_INTRO_STORY_MS = 32
const FLIP_WAIT_PAD_MS = 80
const STORY_LIFT = 'translate3d(0, 16px, 0) scale(0.97)'
/** 网站卡沿轨道入场：前一张从左溢出，其余从右边进来。 */
export const SITE_ENTER = 'translate3d(24px, 0, 0)'
export const SITE_ENTER_LEFT = 'translate3d(-24px, 0, 0)'
/** 与 `.brew-site` / `.is-sites-open .brew-site` 默认透明度对齐。 */
export const SITE_RAIL_OP = 0.64
export const SITE_GRID_OP = 1

export function brewFlipQuiet(): boolean {
  return brewMotionQuiet()
}

export function flipDelay(index: number, lead = false): number {
  if (lead) return 0
  return Math.min(index * FLIP_STAGGER_MS, FLIP_STAGGER_CAP_MS)
}

export function flipDelayFromLead(
  index: number,
  leadIndex: number | null,
): number {
  if (leadIndex == null || leadIndex < 0) return flipDelay(index)
  return flipDelay(Math.abs(index - leadIndex), index === leadIndex)
}

/** 同一列一起走。 */
export function storyColumn(index: number): number {
  return Math.floor(index / 2)
}

export function storyDelay(index: number, extra = 0): number {
  return extra + flipDelay(storyColumn(index))
}

export function siteOpenDelay(story: 'enter' | 'exit'): number {
  return story === 'exit' ? FLIP_STORY_LEAD_MS : 0
}

export function storyPhaseDelay(story: 'enter' | 'exit'): number {
  return story === 'enter' ? FLIP_STORY_FOLLOW_MS : 0
}

export function siteRestOpacity(open: boolean, on: boolean): number {
  if (on) return 1
  return open ? SITE_GRID_OP : SITE_RAIL_OP
}

export interface FeedsChrome {
  airHeight: number
  itemsHeight: number
  sitesHeight: number
}

function chromeBox(root: HTMLElement, selector: string): number {
  return root.querySelector(selector)?.getBoundingClientRect().height ?? 0
}

export function readFeedsChrome(root: HTMLElement): FeedsChrome {
  return {
    airHeight: chromeBox(root, '.brew-feeds__air'),
    itemsHeight: chromeBox(root, '.brew-feeds__items'),
    sitesHeight: chromeBox(root, '.brew-feeds__sites'),
  }
}

const CHROME_STYLE = [
  'transition',
  'flex',
  'height',
  'min-height',
  'opacity',
  'visibility',
  'overflow',
] as const

function pinChromeBox(el: HTMLElement | null, height: number): void {
  if (!el) return
  const h = `${Math.max(0, height)}px`
  el.style.transition = 'none'
  el.style.flex = `0 0 ${h}`
  el.style.height = h
  el.style.minHeight = h
  el.style.overflow = 'visible'
}

export function holdFeedsChrome(root: HTMLElement, chrome: FeedsChrome): void {
  pinChromeBox(root.querySelector('.brew-feeds__air'), chrome.airHeight)
  pinChromeBox(root.querySelector('.brew-feeds__sites'), chrome.sitesHeight)
  const items = root.querySelector<HTMLElement>('.brew-feeds__items')
  pinChromeBox(items, chrome.itemsHeight)
  if (items) {
    items.style.opacity = chrome.itemsHeight > 1 ? '1' : '0'
    items.style.visibility = chrome.itemsHeight > 1 ? 'visible' : 'hidden'
  }
}

export function releaseFeedsChrome(root: HTMLElement): void {
  for (const sel of [
    '.brew-feeds__air',
    '.brew-feeds__sites',
    '.brew-feeds__items',
  ]) {
    const el = root.querySelector<HTMLElement>(sel)
    if (!el) continue
    for (const anim of el.getAnimations()) anim.cancel()
    for (const prop of CHROME_STYLE) el.style.removeProperty(prop)
  }
}

export function playFeedsChrome(
  root: HTMLElement,
  from: FeedsChrome,
  to: FeedsChrome,
): Animation[] {
  const run = (selector: string, a: number, b: number) => {
    const el = root.querySelector<HTMLElement>(selector)
    if (!el) return []
    if (Math.abs(a - b) < 0.6) return []
    return [
      play(
        el,
        [
          { height: `${a}px`, minHeight: `${a}px`, flexBasis: `${a}px` },
          { height: `${b}px`, minHeight: `${b}px`, flexBasis: `${b}px` },
        ],
        0,
        FLIP_SITE_MS,
      ),
    ]
  }
  return [
    ...run('.brew-feeds__air', from.airHeight, to.airHeight),
    ...run('.brew-feeds__items', from.itemsHeight, to.itemsHeight),
    ...run('.brew-feeds__sites', from.sitesHeight, to.sitesHeight),
  ]
}

export function flipWaitMs(): number {
  const openEnd = FLIP_STORY_LEAD_MS + FLIP_SITE_MS + FLIP_STAGGER_CAP_MS
  const foldEnd = FLIP_STORY_FOLLOW_MS + FLIP_STORY_MS + FLIP_STAGGER_CAP_MS
  return Math.max(openEnd, foldEnd) + FLIP_WAIT_PAD_MS
}

export interface FlipBox {
  left: number
  top: number
  width: number
  height: number
  opacity: number
}

function liveCards(root: ParentNode, selector: string): HTMLElement[] {
  return Iterator.from(root.querySelectorAll<HTMLElement>(selector))
    .filter((el) => !el.dataset.brewGhost)
    .toArray()
}

export function readOpacity(el: Element): number {
  const n = Number(getComputedStyle(el).opacity)
  return Number.isFinite(n) ? n : 1
}

export function readFlipBoxes(
  root: ParentNode,
  selector: string,
): Map<string, FlipBox> {
  const boxes = new Map<string, FlipBox>()
  for (const el of liveCards(root, selector)) {
    const id = el.dataset.railId
    if (!id) continue
    const rect = el.getBoundingClientRect()
    boxes.set(id, {
      left: rect.left,
      top: rect.top,
      width: rect.width,
      height: rect.height,
      opacity: readOpacity(el),
    })
  }
  return boxes
}

function clearRailExit(el: HTMLElement): void {
  delete el.dataset.leaving
  el.style.visibility = ''
  el.style.pointerEvents = ''
  el.style.removeProperty('--exit')
  el.style.removeProperty('--exit-x')
}

export function clearRailExits(root: ParentNode, selector: string): void {
  for (const el of root.querySelectorAll<HTMLElement>(selector)) {
    clearRailExit(el)
  }
}

export function seatSiteTrack(track: HTMLElement, id: number): void {
  const cards = Iterator.from(
    track.querySelectorAll<HTMLElement>('.brew-site'),
  )
    .map((el) => ({ el, left: el.offsetLeft }))
    .toArray()
  const index = cards.findIndex((card) => Number(card.el.dataset.railId) === id)
  const x = railSeatScroll(cards, index, RAIL_OVERFLOW_LEFT_PX)
  track.style.transform = x > 0.5 ? `translate3d(${-x}px, 0, 0)` : ''
}

function play(
  el: HTMLElement,
  keyframes: Keyframe[],
  delay: number,
  duration: number,
): Animation {
  return el.animate(keyframes, {
    duration,
    delay,
    easing: FLIP_EASE,
    fill: 'both',
    composite: 'replace',
  })
}

export function flipFromBoxes(
  root: ParentNode,
  selector: string,
  first: Map<string, FlipBox>,
  leadId?: string | null,
  delayExtra = 0,
): Animation[] {
  const anims: Animation[] = []
  const cards = liveCards(root, selector)
  const leadIndex = leadId
    ? cards.findIndex((el) => el.dataset.railId === leadId)
    : -1
  cards.forEach((el, index) => {
    const id = el.dataset.railId
    if (!id) return
    const prev = first.get(id)
    const next = el.getBoundingClientRect()
    const toOp = readOpacity(el)
    const delay = delayExtra + flipDelayFromLead(index, leadIndex)
    if (!prev) {
      const from = index < leadIndex ? SITE_ENTER_LEFT : SITE_ENTER
      anims.push(
        play(
          el,
          [
            { opacity: toOp, transform: from },
            { opacity: toOp, transform: 'translate3d(0, 0, 0)' },
          ],
          delay,
          FLIP_SITE_MS,
        ),
      )
      return
    }
    const dx = prev.left - next.left
    const dy = prev.top - next.top
    const fromOp = prev.opacity
    if (Math.abs(dx) < 0.6 && Math.abs(dy) < 0.6 && Math.abs(fromOp - toOp) < 0.02) {
      return
    }
    anims.push(
      play(
        el,
        [
          {
            opacity: fromOp,
            transform: `translate3d(${dx}px, ${dy}px, 0)`,
          },
          { opacity: toOp, transform: 'translate3d(0, 0, 0)' },
        ],
        delay,
        FLIP_SITE_MS,
      ),
    )
  })
  return anims
}

/** 真卡保持隐藏，避免宫格先闪一帧。 */
export function flySites(
  root: HTMLElement,
  first: Map<string, FlipBox>,
  leadId?: string | null,
  delayExtra = 0,
  open = false,
): Animation[] {
  const cards = liveCards(root, '.brew-site')
  const leadIndex = leadId
    ? cards.findIndex((el) => el.dataset.railId === leadId)
    : -1
  const hostBox = root.getBoundingClientRect()
  const next = cards.map((el) => {
    const rect = el.getBoundingClientRect()
    const on =
      el.classList.contains('is-on') || el.classList.contains('is-cover')
    return {
      el,
      id: el.dataset.railId ?? '',
      box: {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      },
      on,
      toOp: siteRestOpacity(open, on),
    }
  })
  const anims: Animation[] = []
  next.forEach((item, index) => {
    if (!item.id) return
    const prev = first.get(item.id)
    const delay = delayExtra + flipDelayFromLead(index, leadIndex)
    let ghost = findGhost(root, 'site', item.id)
    if (!ghost && prev) {
      ghost = liftCard(item.el, prev, root, hostBox, 'site')
    }
    if (!ghost) {
      item.el.classList.add('is-ghosted')
      item.el.style.visibility = 'hidden'
      return
    }
    const dx = item.box.left - (prev?.left ?? item.box.left)
    const dy = item.box.top - (prev?.top ?? item.box.top)
    const fromOp =
      prev && prev.opacity > 0.05
        ? prev.opacity
        : siteRestOpacity(!open, item.on)
    anims.push(
      play(
        ghost,
        [
          { opacity: fromOp, transform: 'translate3d(0, 0, 0)' },
          {
            opacity: item.toOp,
            transform: `translate3d(${dx}px, ${dy}px, 0)`,
          },
        ],
        delay,
        FLIP_SITE_MS,
      ),
    )
  })
  return anims
}

export function pinFlipBox(
  el: HTMLElement,
  box: Pick<FlipBox, 'left' | 'top' | 'width' | 'height'>,
  host?: DOMRect | null,
): void {
  const left = host ? box.left - host.left : box.left
  const top = host ? box.top - host.top : box.top
  el.style.position = host ? 'absolute' : 'fixed'
  el.style.left = `${left}px`
  el.style.top = `${top}px`
  el.style.width = `${box.width}px`
  el.style.height = `${box.height}px`
  el.style.zIndex = '8'
  el.style.margin = '0'
  el.style.boxSizing = 'border-box'
}

export function unpinFlip(el: HTMLElement): void {
  el.style.removeProperty('position')
  el.style.removeProperty('left')
  el.style.removeProperty('top')
  el.style.removeProperty('width')
  el.style.removeProperty('height')
  el.style.removeProperty('z-index')
  el.style.removeProperty('margin')
  el.style.removeProperty('box-sizing')
}

function liftCard(
  el: HTMLElement,
  box: FlipBox,
  host: HTMLElement,
  hostBox: DOMRect,
  kind: 'site' | 'story',
): HTMLElement {
  const id = el.dataset.railId ?? ''
  const ghost = el.cloneNode(true) as HTMLElement
  ghost.dataset.brewGhost = kind
  if (id) ghost.dataset.brewFrom = id
  ghost.removeAttribute('data-rail-id')
  ghost.setAttribute('aria-hidden', 'true')
  pinFlipBox(ghost, box, hostBox)
  ghost.style.zIndex = '12'
  host.appendChild(ghost)
  el.classList.add('is-ghosted')
  el.style.visibility = 'hidden'
  return ghost
}

/** 换布局前钉在原位，避免先上屏再追。 */
export function liftCards(
  root: HTMLElement,
  selector: string,
  first: Map<string, FlipBox>,
  kind: 'site' | 'story',
): HTMLElement[] {
  const hostBox = root.getBoundingClientRect()
  const ghosts: HTMLElement[] = []
  for (const el of liveCards(root, selector)) {
    const id = el.dataset.railId
    const box = id ? first.get(id) : undefined
    if (!box) {
      el.classList.add('is-ghosted')
      el.style.visibility = 'hidden'
      continue
    }
    ghosts.push(liftCard(el, box, root, hostBox, kind))
  }
  return ghosts
}

function findGhost(
  root: ParentNode,
  kind: 'site' | 'story',
  id: string,
): HTMLElement | null {
  return root.querySelector(
    `[data-brew-ghost="${kind}"][data-brew-from="${CSS.escape(id)}"]`,
  )
}

export function clearStoryLifts(root: ParentNode): void {
  const feeds =
    root instanceof HTMLElement && root.classList.contains('brew-feeds')
      ? root
      : root instanceof Element
        ? root.closest('.brew-feeds')
        : null
  const open = !!feeds?.classList.contains('is-sites-open')
  // 先露活卡再拆幽灵，先拆会空一帧。
  for (const el of root.querySelectorAll<HTMLElement>(
    '.is-ghosted, .brew-site, .brew-story',
  )) {
    if (el.dataset.brewGhost) continue
    for (const anim of el.getAnimations()) anim.cancel()
    el.classList.remove('is-ghosted')
    el.style.transition = 'none'
    el.style.removeProperty('visibility')
    el.style.removeProperty('transform')
    if (el.classList.contains('brew-site')) {
      const on =
        el.classList.contains('is-on') || el.classList.contains('is-cover')
      el.style.opacity = String(siteRestOpacity(open, on))
    } else {
      el.style.removeProperty('opacity')
    }
  }
  for (const ghost of root.querySelectorAll('[data-brew-ghost]')) {
    ghost.remove()
  }
  if (feeds instanceof HTMLElement) {
    feeds.classList.add('is-sites-settling')
    requestAnimationFrame(() => {
      for (const el of liveCards(feeds, '.brew-site')) {
        el.style.removeProperty('opacity')
        el.style.removeProperty('transition')
      }
      for (const el of liveCards(feeds, '.brew-story')) {
        el.style.removeProperty('transition')
      }
      feeds.classList.remove('is-sites-settling')
    })
  }
}

export function exitStories(
  root: ParentNode,
  selector: string,
  first?: Map<string, FlipBox> | null,
  host?: HTMLElement | null,
): Animation[] {
  const hostBox = host?.getBoundingClientRect() ?? null
  return liveCards(root, selector).map((el, index) => {
    const id = el.dataset.railId
    const prev = id && first ? first.get(id) : undefined
    const existing = id ? findGhost(root, 'story', id) : null
    const target =
      existing ??
      (host && hostBox && prev ? liftCard(el, prev, host, hostBox, 'story') : el)
    const fromOp = prev?.opacity ?? readOpacity(el)
    return play(
      target,
      [
        { opacity: fromOp, transform: 'translate3d(0, 0, 0) scale(1)' },
        { opacity: 0, transform: STORY_LIFT },
      ],
      storyDelay(index),
      FLIP_STORY_MS,
    )
  })
}

export function enterCards(
  root: ParentNode,
  selector: string,
  extraDelay = 0,
  duration = FLIP_STORY_MS,
  toOp = 1,
): Animation[] {
  return liveCards(root, selector).map((el, index) => {
    return play(
      el,
      [
        { opacity: 0, transform: STORY_LIFT },
        { opacity: toOp, transform: 'translate3d(0, 0, 0) scale(1)' },
      ],
      storyDelay(index, extraDelay),
      duration,
    )
  })
}

export function enterStories(
  root: ParentNode,
  selector: string,
  extraDelay = FLIP_STORY_FOLLOW_MS,
): Animation[] {
  return enterCards(root, selector, extraDelay, FLIP_STORY_MS)
}

export function enterSites(
  root: ParentNode,
  extraDelay = 0,
  leadId?: string | null,
): Animation[] {
  const cards = liveCards(root, '.brew-site')
  const leadIndex = leadId
    ? cards.findIndex((el) => el.dataset.railId === leadId)
    : 0
  return cards.map((el, index) => {
    const on =
      el.classList.contains('is-on') || el.classList.contains('is-cover')
    const op = siteRestOpacity(false, on)
    const from = index < leadIndex ? SITE_ENTER_LEFT : SITE_ENTER
    return play(
      el,
      [
        { opacity: 0, transform: from },
        { opacity: op, transform: 'translate3d(0, 0, 0)' },
      ],
      extraDelay + flipDelayFromLead(index, leadIndex < 0 ? 0 : leadIndex),
      FLIP_SITE_MS,
    )
  })
}

function animTarget(anim: Animation): HTMLElement | null {
  const effect = anim.effect
  if (!effect || !('target' in effect)) return null
  const target = (effect as KeyframeEffect).target
  return target instanceof HTMLElement ? target : null
}

export function waitFlip(_anims: readonly Animation[]): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, flipWaitMs())
  })
}

export function settleFlip(anims: readonly Animation[]): void {
  for (const anim of anims) {
    try {
      if (anim.playState !== 'finished') anim.finish()
    } catch {
      anim.cancel()
    }
  }
}

export function dropStaleGhosts(root: ParentNode): void {
  for (const ghost of root.querySelectorAll('[data-brew-ghost]')) {
    ghost.remove()
  }
}

let feedsRevealHandler: (() => void) | null = null

export function setFeedsRevealHandler(fn: (() => void) | null): void {
  feedsRevealHandler = fn
}

/** 换树前揭回活卡，避免幽灵留在退场里。 */
export function revealFeedsTree(root?: ParentNode | null): void {
  if (!root) return
  const feeds =
    root instanceof HTMLElement && root.classList.contains('brew-feeds')
      ? root
      : root instanceof Element
        ? root.querySelector('.brew-feeds')
        : null
  if (!(feeds instanceof HTMLElement)) return
  feeds.classList.remove('is-sites-morphing')
  clearStoryLifts(feeds)
  releaseFeedsChrome(feeds)
  feedsRevealHandler?.()
}

export function dropFlip(
  anims: readonly Animation[],
  root?: ParentNode | null,
): void {
  for (const anim of anims) {
    anim.cancel()
    const el = animTarget(anim)
    if (!el) continue
    if (el.dataset.brewGhost) continue
    el.style.removeProperty('transform')
    el.style.removeProperty('opacity')
    unpinFlip(el)
  }
  if (root) revealFeedsTree(root)
}
