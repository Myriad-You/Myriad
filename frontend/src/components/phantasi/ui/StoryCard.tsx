/** 不认识 phantasi_items。 */

import type {
  CSSProperties,
  FocusEvent as ReactFocusEvent,
  PointerEvent as ReactPointerEvent,
  Ref,
  SyntheticEvent,
} from 'react'

import type { PeekStoryPreview } from './peekLane'
import { forwardRef, useCallback, useRef } from 'react'
import { scheduleTask } from '../../../hooks/animation'
import { phantasiMotionBusy, whenPhantasiMotionIdle } from '../../../hooks/animation/pages/phantasiMotion'
import { isPlainClick } from '../../../utils/plainClick'
import { cx } from './cx'
import { canPhantasiPeek } from './interactionMedia'
import {
  notePeekPointer,
  peekLaneIsLive,
  peekLaneKeepsAir,
  peekNodeFromPoint,
  peekPreviewFromStory,
  peekStoryNode,
  resetPeekPointer,
} from './peekLane'
import { PhantasiPick } from './Pick'

function hideBrokenSourceIcon(
  event: SyntheticEvent<HTMLImageElement>,
): void {
  event.currentTarget.hidden = true
}

export function markPhantasiStoryPeek(target: EventTarget | null, on: boolean): void {
  const node = peekStoryNode(target)
  if (!node) return
  if (on) {
    clearPhantasiStoryPeeks()
    node.classList.add('is-peek')
    return
  }
  node.classList.remove('is-peek')
}

export function releasePhantasiStoryPeek(
  target: EventTarget | null,
  related: EventTarget | null,
  onEnd?: () => void,
): void {
  markPhantasiStoryPeek(target, false)
  if (!peekLaneIsLive(target)) return
  if (peekLaneKeepsAir(target, related)) return
  onEnd?.()
}

export function dropPhantasiPeekLane(
  root: EventTarget | null,
  related: EventTarget | null,
  onEnd?: () => void,
): void {
  const host =
    root && typeof (root as Element).querySelectorAll === 'function'
      ? (root as Element)
      : null
  clearPhantasiStoryPeeks(host)
  if (!peekLaneIsLive(root)) return
  if (peekLaneKeepsAir(root, related)) return
  onEnd?.()
}

export function resumePhantasiStoryPeek(
  onPeek?: (item: PeekStoryPreview) => void,
): boolean {
  if (!canPhantasiPeek() || phantasiMotionBusy()) return false
  const node = peekNodeFromPoint()
  if (!node) return false
  markPhantasiStoryPeek(node, true)
  const item = peekPreviewFromStory(node)
  if (item) onPeek?.(item)
  return true
}

let peekResumeGen = 0
let peekResumeStop: (() => void) | null = null

export function cancelPhantasiPeekResume(): void {
  peekResumeGen += 1
  peekResumeStop?.()
  peekResumeStop = null
}

export function usePhantasiPeekLane({
  onPeek,
  onPeekEnd,
  blocked,
}: {
  onPeek?: (item: PeekStoryPreview) => void
  onPeekEnd?: () => void
  blocked?: () => boolean
}): {
  onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void
  onPointerOver: (event: ReactPointerEvent<HTMLElement>) => void
  onPointerOut: (event: ReactPointerEvent<HTMLElement>) => void
  onPointerLeave: (event: ReactPointerEvent<HTMLElement>) => void
  onPointerCancel: (event: ReactPointerEvent<HTMLElement>) => void
  onFocus: (event: ReactFocusEvent<HTMLElement>) => void
  onBlur: (event: ReactFocusEvent<HTMLElement>) => void
} {
  const onPeekRef = useRef(onPeek)
  onPeekRef.current = onPeek
  const onPeekEndRef = useRef(onPeekEnd)
  onPeekEndRef.current = onPeekEnd
  const blockedRef = useRef(blocked)
  blockedRef.current = blocked

  const onPointerOver = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    notePeekPointer(event)
    if (!canPhantasiPeek() || event.pointerType === 'touch' || event.buttons || blockedRef.current?.()) return
    const node = peekStoryNode(event.target)
    if (!node || !peekLaneIsLive(node) || phantasiMotionBusy()) return
    if (node.classList.contains('is-peek')) return
    markPhantasiStoryPeek(node, true)
    const item = peekPreviewFromStory(node)
    if (item) onPeekRef.current?.(item)
  }, [])

  const onPointerOut = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    const node = peekStoryNode(event.target)
    if (node && event.relatedTarget instanceof Node && node.contains(event.relatedTarget)) return
    releasePhantasiStoryPeek(node, event.relatedTarget, onPeekEndRef.current)
  }, [])

  const onPointerLeave = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    dropPhantasiPeekLane(event.currentTarget, event.relatedTarget, onPeekEndRef.current)
  }, [])

  const onPointerCancel = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    resetPeekPointer()
    dropPhantasiPeekLane(event.currentTarget, null, onPeekEndRef.current)
  }, [])

  const onFocus = useCallback((event: ReactFocusEvent<HTMLElement>) => {
    if (!canPhantasiPeek() || blockedRef.current?.()) return
    const node = peekStoryNode(event.target)
    // A touch/click focus must not manufacture hover. Keyboard focus is explicit.
    if (!node || !peekLaneIsLive(node) || !node.matches(':focus-visible')) return
    markPhantasiStoryPeek(node, true)
    const item = peekPreviewFromStory(node)
    if (item) onPeekRef.current?.(item)
  }, [])

  const onBlur = useCallback((event: ReactFocusEvent<HTMLElement>) => {
    const node = peekStoryNode(event.target)
    if (node && event.relatedTarget instanceof Node && node.contains(event.relatedTarget)) return
    releasePhantasiStoryPeek(node, event.relatedTarget, onPeekEndRef.current)
  }, [])

  return {
    onPointerOver,
    onPointerMove: onPointerOver,
    onPointerOut,
    onPointerLeave,
    onPointerCancel,
    onFocus,
    onBlur,
  }
}

export function schedulePhantasiPeekResume(
  onPeek: (item: PeekStoryPreview) => void,
): void {
  cancelPhantasiPeekResume()
  const gen = peekResumeGen
  peekResumeStop = whenPhantasiMotionIdle(() => {
    if (gen !== peekResumeGen) return
    if (resumePhantasiStoryPeek(onPeek)) {
      cancelPhantasiPeekResume()
      return
    }
    scheduleTask(() => {
      if (gen !== peekResumeGen) return
      resumePhantasiStoryPeek(onPeek)
    })
  })
}

export function clearPhantasiStoryPeeks(root?: ParentNode | null): void {
  const scope = root ?? (typeof document === 'undefined' ? null : document)
  scope?.querySelectorAll('.phantasi-story.is-peek').forEach((node) => {
    node.classList.remove('is-peek')
  })
}

function syncStoryCoverClass(story: Element | null, coverOk: boolean): void {
  if (!(story instanceof HTMLElement)) return
  if (coverOk) {
    story.classList.add('has-cover')
    return
  }
  story.classList.remove('has-cover')
}

function hideBrokenStoryCover(
  event: SyntheticEvent<HTMLImageElement>,
): void {
  const thumb = event.currentTarget.closest('.phantasi-story__thumb')
  if (thumb instanceof HTMLElement) thumb.hidden = true
  syncStoryCoverClass(event.currentTarget.closest('.phantasi-story'), false)
}

function showLoadedStoryCover(event: SyntheticEvent<HTMLImageElement>): void {
  const thumb = event.currentTarget.closest('.phantasi-story__thumb')
  if (thumb instanceof HTMLElement) thumb.hidden = false
  syncStoryCoverClass(event.currentTarget.closest('.phantasi-story'), true)
}

const placeAbs = new Map<string, CSSProperties>()
const storyClass = new Map<number, string>()

function storyCardClass(
  unread: boolean,
  cover: boolean,
  star: boolean,
  hold: boolean,
): string {
  const key =
    (unread ? 1 : 0)
    | (cover ? 2 : 0)
    | (star ? 4 : 0)
    | (hold ? 8 : 0)
  const hit = storyClass.get(key)
  if (hit) return hit
  const next = cx(
    'phantasi-story phantasi-float phantasi-story__hit phantasi-story__shell',
    unread && 'is-unread',
    cover && 'has-cover',
    star && 'has-star',
    hold && 'is-hold',
  )
  storyClass.set(key, next)
  return next
}

function storyPlaceStyle(col: number, row: 1 | 2): CSSProperties {
  const key = `${col}:${row}`
  const hit = placeAbs.get(key)
  if (hit) return hit
  const style: CSSProperties = {
    position: 'absolute',
    left: `calc(${col - 1} * (var(--phantasi-story-w) + 0.75rem))`,
    top: row === 2
      ? 'calc(var(--phantasi-story-h) + var(--phantasi-items-gap))'
      : 0,
    width: 'var(--phantasi-story-w)',
    height: 'var(--phantasi-story-h)',
  }
  placeAbs.set(key, style)
  return style
}

interface StoryCardFace {
  id: number | string
  title: string
  summary?: string
  cover?: string | null
  source?: string
  sourceIcon?: string | null
  when?: string
  topic?: string | null
  hue?: string | null
  author?: string | null
  unread?: boolean
  starred?: boolean
}

export const StoryCard = forwardRef<
  HTMLButtonElement,
  {
    face?: StoryCardFace
    /** 有地址时卡片是真链接：右键复制、中键新开、长按都归浏览器；普通左键仍走 onOpen。 */
    href?: string
    unreadLabel?: string
    starLabel?: string
    unstarLabel?: string
    onOpen?: () => void
    shell?: boolean
    onPeek?: () => void
    onPeekEnd?: () => void
    onToggleStar?: () => void
    current?: boolean
    picked?: boolean
    picking?: boolean
    arrive?: number
    railId?: number | string
    place?: { column: number; row: 1 | 2 }
    railCol?: number
    eagerCover?: boolean
    holdCover?: boolean
    canStar?: boolean
    html?: string
  }
>((
  {
    face,
    href,
    unreadLabel,
    starLabel,
    unstarLabel,
    onOpen,
    onPeek,
    onPeekEnd,
    onToggleStar,
    current = false,
    picked = false,
    picking = false,
    arrive,
    railId,
    place,
    railCol,
    eagerCover = false,
    holdCover = false,
    canStar,
    html,
    shell = false,
  },
  ref,
) => {
  const placed =
    railCol != null && place
      ? storyPlaceStyle(railCol, place.row)
      : place
        ? { gridColumn: place.column, gridRow: place.row }
        : null
  if (shell || !face) {
    return (
      <button
        ref={ref}
        type="button"
        data-rail-id={railId}
        data-rail-col={railCol ?? place?.column}
        data-phantasi-surface="story"
        className="phantasi-story phantasi-story--slot"
        style={placed ?? undefined}
        aria-hidden
        tabIndex={-1}
      />
    )
  }
  const style = (
    face.hue || arrive != null
      ? {
          ...(face.hue ? { '--story-topic': face.hue } : null),
          ...(arrive != null ? { '--phantasi-card-i': arrive } : null),
          ...placed,
        }
      : placed
  ) as CSSProperties | null
  const deferCover = holdCover && !eagerCover
  const showStar = canStar ?? !!onToggleStar
  if (
    html
    && !onOpen
    && !onPeek
    && !onPeekEnd
    && !onToggleStar
    && !picking
    && !current
    && !picked
    && arrive == null
  ) {
    const fastClass = storyCardClass(
      !!face.unread,
      !!face.cover,
      showStar,
      deferCover,
    )
    // 点击由轨道代理（它负责拦下普通左键）；这里只把地址交给浏览器。
    if (href) {
      return (
        <a
          ref={ref as Ref<HTMLAnchorElement>}
          href={href}
          draggable={false}
          data-rail-id={railId}
          data-rail-col={railCol ?? place?.column}
          data-phantasi-surface="story"
          className={fastClass}
          style={style ?? undefined}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      )
    }
    return (
      <button
        ref={ref}
        type="button"
        data-rail-id={railId}
        data-rail-col={railCol ?? place?.column}
        data-phantasi-surface="story"
        className={fastClass}
        style={style ?? undefined}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    )
  }

  const content = (
    <>
        <span className="phantasi-story__kicker">
          {face.topic ? (
            <span className="phantasi-story__topic">{face.topic}</span>
          ) : null}
          {face.source || face.sourceIcon ? (
            <span
              className="phantasi-story__source"
              {...(deferCover && face.sourceIcon
                ? { 'data-src': face.sourceIcon }
                : {})}
            >
              {face.sourceIcon && !deferCover ? (
                <img
                  draggable={false}
                  key={face.sourceIcon}
                  src={face.sourceIcon}
                  data-src={face.sourceIcon}
                  alt=""
                  loading="lazy"
                  decoding="async"
                  onError={hideBrokenSourceIcon}
                />
              ) : !face.sourceIcon && face.source ? (
                <span className="phantasi-story__source-mark" aria-hidden>
                  {face.source.slice(0, 1)}
                </span>
              ) : null}
              {face.source ? <span>{face.source}</span> : null}
            </span>
          ) : null}
          {face.when ? (
            <span className="phantasi-story__meta">{face.when}</span>
          ) : null}
          {face.unread ? (
            <span className="phantasi-story__unread">{unreadLabel}</span>
          ) : null}
        </span>
        <span className="phantasi-story__title">{face.title}</span>
        {face.author ? (
          <span className="phantasi-story__byline">{face.author}</span>
        ) : null}
        {face.cover ? (
          <span
            key={face.cover}
            className="phantasi-story__thumb"
            aria-hidden
            {...(deferCover ? { 'data-src': face.cover } : {})}
          >
            {deferCover ? null : (
              <img
                draggable={false}
                src={face.cover}
                data-src={face.cover}
                alt=""
                loading={eagerCover ? 'eager' : 'lazy'}
                decoding="async"
                onError={hideBrokenStoryCover}
                onLoad={showLoadedStoryCover}
              />
            )}
          </span>
        ) : null}
        {face.summary ? (
          <span className="phantasi-story__summary">{face.summary}</span>
        ) : null}
      {picking ? (
        <PhantasiPick on={picked} />
      ) : showStar ? (
        <span
          className={cx('phantasi-story__star', face.starred && 'is-on')}
          title={face.starred ? unstarLabel : starLabel}
          aria-label={face.starred ? unstarLabel : starLabel}
          aria-pressed={!!face.starred}
          role="button"
          onClick={
            onToggleStar
              ? (event) => {
                  // 卡片可能是链接：只拦冒泡不够，还要拦下链接跳转。
                  event.preventDefault()
                  event.stopPropagation()
                  onToggleStar()
                }
              : undefined
          }
        />
      ) : null}
    </>
  )
  const className =
    current || picked || picking || arrive != null
      ? cx(
          storyCardClass(
            !!face.unread,
            !!face.cover,
            showStar && !picking,
            deferCover,
          ),
          current && 'is-current',
          picked && 'is-picked',
          picking && 'is-picking',
          arrive != null && 'is-arrive',
        )
      : storyCardClass(
          !!face.unread,
          !!face.cover,
          showStar,
          deferCover,
        )
  // 挑选（编辑）态也保持 <a>：元素类型一变整张卡会重挂，入场动画重播、封面重载。
  // 挑选时它是个开关：任何点击都只切换选中，不跳转。
  if (href) {
    return (
      <a
        ref={ref as Ref<HTMLAnchorElement>}
        href={href}
        draggable={false}
        role={picking ? 'button' : undefined}
        aria-pressed={picking ? picked : undefined}
        data-rail-id={railId}
        data-rail-col={railCol ?? place?.column}
        data-phantasi-surface="story"
        data-phantasi-card={arrive != null ? `story:${face.id}` : undefined}
        className={className}
        style={style ?? undefined}
        onClick={(event) => {
          if (picking) {
            event.preventDefault()
            onOpen?.()
            return
          }
          if (!onOpen || !isPlainClick(event)) return
          event.preventDefault()
          onOpen()
        }}
      >
        {content}
      </a>
    )
  }
  return (
    <button
      ref={ref}
      type="button"
      data-rail-id={railId}
      data-rail-col={railCol ?? place?.column}
      data-phantasi-surface="story"
      data-phantasi-card={arrive != null ? `story:${face.id}` : undefined}
      className={className}
      style={style ?? undefined}
      onClick={onOpen}
      aria-pressed={picking ? picked : undefined}
    >
      {content}
    </button>
  )
})
