import type { RefObject } from 'react'
import type { StageMedia } from './stageMedia'
import type { StageLink } from './useStageLinks'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react'

/** 当前这句一段一段浮出来，整句最多花这么久，长句每段更快。 */
const REVEAL_TOTAL_MS = 720
const REVEAL_STEP_MAX_MS = 55

const wordSegmenter =
  typeof Intl !== 'undefined' && 'Segmenter' in Intl
    ? new Intl.Segmenter(undefined, { granularity: 'word' })
    : null

const SPACE = /^\s+$/
const PUNCTUATION = /^\p{P}+$/u
/** 开括号、开引号：跟后面的词走，不能留在行尾。 */
const OPENING = /^[「『（([{“‘《〈【〔]+$/u

/**
 * 按词切：中日文切成词，西文按单词。每段是一个 inline-block，才能上浮；
 * 按字切的话西文单词会在中间断行。空白原样留成文字，照常在那里换行。
 *
 * 标点不单独成段：单独一块的话浏览器的避头尾管不到它，「、」「，」会跑到行首。
 * 开括号（以及行首、空格后的标点）并进后一段，其余并进前一段。
 */
function segmentsOf(text: string): { text: string; space: boolean }[] {
  const parts = wordSegmenter
    ? Array.from(wordSegmenter.segment(text), (part) => part.segment)
    : text.split(/(\s+)/).filter(Boolean)
  const segments: { text: string; space: boolean }[] = []
  let opening = ''
  for (const part of parts) {
    if (SPACE.test(part)) {
      if (opening) segments.push({ text: opening, space: false })
      opening = ''
      segments.push({ text: part, space: true })
      continue
    }
    const previous = segments.at(-1)
    // 行首或空格后的标点（含西文直引号）也是开头的那种。
    if (OPENING.test(part) || (PUNCTUATION.test(part) && (!previous || previous.space))) {
      opening += part
      continue
    }
    if (!opening && PUNCTUATION.test(part) && previous && !previous.space) {
      previous.text += part
      continue
    }
    segments.push({ text: opening + part, space: false })
    opening = ''
  }
  if (opening) segments.push({ text: opening, space: false })
  return segments
}

/** 当前这句。屏幕阅读器读整句，逐段的那层只给眼睛看。 */
export function RevealLine({ text, animate }: { text: string; animate: boolean }) {
  const segments = useMemo(() => segmentsOf(text), [text])
  if (!animate) return <>{text}</>
  const words = segments.filter((part) => !part.space).length
  const step = Math.min(REVEAL_STEP_MAX_MS, REVEAL_TOTAL_MS / Math.max(1, words))
  let index = 0
  return (
    <>
      <span className="report-stage__sr">{text}</span>
      <span aria-hidden="true">
        {segments.map((part, key) =>
          part.space ? (
            part.text
          ) : (
            <span
              // 段落不会重排，用位置当 key 就够了。
              key={key}
              className="report-stage__word"
              style={{ animationDelay: `${Math.round(index++ * step)}ms` }}
            >
              {part.text}
            </span>
          ),
        )}
      </span>
    </>
  )
}

const RING_RADIUS = 27
const RING_LENGTH = 2 * Math.PI * RING_RADIUS

/** 播放键外圈：整份报告播到哪了。每幕一段，段间留缝。 */
export function ProgressRing({
  chapters,
  chapter,
  fraction,
}: {
  /** 每幕几句。 */
  chapters: readonly number[]
  chapter: number
  /** 这一幕播了几成。 */
  fraction: number
}) {
  const count = Math.max(1, chapters.length)
  const gap = count > 1 ? 5 : 0
  const segment = RING_LENGTH / count
  return (
    <svg className="report-stage__ring" viewBox="0 0 60 60" aria-hidden="true">
      {Array.from({ length: count }, (_, index) => {
        const filled = index < chapter ? 1 : index === chapter ? fraction : 0
        const length = Math.max(0, segment - gap)
        return (
          <g key={index} style={{ transform: `rotate(${(index * 360) / count - 90}deg)`, transformOrigin: '30px 30px' }}>
            <circle className="report-stage__ring-track" cx="30" cy="30" r={RING_RADIUS} strokeDasharray={`${length} ${RING_LENGTH}`} />
            <circle
              className="report-stage__ring-fill"
              cx="30"
              cy="30"
              r={RING_RADIUS}
              strokeDasharray={`${length * filled} ${RING_LENGTH}`}
            />
          </g>
        )
      })}
    </svg>
  )
}

/** 播放键用的细字形：图标库里的实心款太重，描边款的暂停又是两个空框。 */
export function StagePauseIcon(props: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true" {...props}>
      <rect x="4.25" y="3" width="2" height="10" rx="1" />
      <rect x="9.75" y="3" width="2" height="10" rx="1" />
    </svg>
  )
}

export function StagePlayIcon(props: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true" {...props}>
      <path d="M5 3.9c0-.8.87-1.28 1.54-.85l6.1 4.1c.6.4.6 1.3 0 1.7l-6.1 4.1C5.87 13.38 5 12.9 5 12.1Z" />
    </svg>
  )
}

/**
 * 字幕下面那排封面。台词点到名时是提到的那几张，否则是这个平台的书架。
 * 能点的去原站或用站内播放器放；点封面不翻字幕。点不了的那张屏幕阅读器不读：
 * 作品名台词里已经有了。
 */
export function StageCovers({
  items,
  mode,
  links,
  labels,
  onPlay,
}: {
  items: readonly StageMedia[]
  mode: 'mentioned' | 'shelf'
  links: ReadonlyMap<string, StageLink>
  labels: { play: (title: string) => string; open: (title: string) => string }
  onPlay: (item: StageLink & { kind: 'play' }) => void
}) {
  if (items.length === 0) return null
  return (
    <div className="report-stage__covers" data-mode={mode} data-flip="covers" aria-live="off">
      {items.map((item, index) => {
        const link = links.get(item.cover)
        const body = (
          <>
            <span className="report-stage__cover-art">
              <img
                src={item.cover}
                alt=""
                loading="lazy"
                decoding="async"
                // 封面取不到就整张不显示，不留破图。
                onError={(event) => {
                  const cover = event.currentTarget.closest<HTMLElement>('.report-stage__cover')
                  if (cover) cover.hidden = true
                }}
              />
              {link?.kind === 'play' ? <StagePlayIcon className="report-stage__cover-play" /> : null}
            </span>
            <span className="report-stage__cover-title">{item.title}</span>
          </>
        )
        const common = {
          className: 'report-stage__cover',
          'data-shape': item.shape,
          style: { animationDelay: `${index * 70}ms` },
        }
        if (link?.kind === 'open') {
          return (
            <a
              key={item.cover}
              {...common}
              href={link.href}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={labels.open(item.title)}
              onClick={(event) => event.stopPropagation()}
            >
              {body}
            </a>
          )
        }
        if (link?.kind === 'play') {
          return (
            <button
              key={item.cover}
              {...common}
              type="button"
              aria-label={labels.play(item.title)}
              onClick={(event) => {
                event.stopPropagation()
                onPlay(link)
              }}
            >
              {body}
            </button>
          )
        }
        return (
          <span key={item.cover} {...common} aria-hidden="true">
            {body}
          </span>
        )
      })}
    </div>
  )
}

const FLIP_MS = 620
const FLIP_EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'

interface Flight {
  /** 起点相对终点的位移。 */
  dy: number
  /** 起点的缩放（字号变了的那句：旧字号 / 新字号）。 */
  scale: number
}
const flights = new WeakMap<Animation, Flight>()

interface Placement {
  top: number
  font: number
}

/** 元素在 root 里的布局位置（不含 transform），沿 offsetParent 一路加上去。 */
function layoutTop(element: HTMLElement, root: HTMLElement): number {
  let top = 0
  let node: HTMLElement | null = element
  while (node && node !== root) {
    top += node.offsetTop
    node = node.offsetParent as HTMLElement | null
  }
  return top
}

/** 还在走的那段此刻停在哪：离终点的位移、当前缩放。读完就停掉，由新的一段接着走。 */
function takeFlight(element: HTMLElement): Flight {
  let dy = 0
  let scale = 1
  for (const animation of element.getAnimations()) {
    const flight = flights.get(animation)
    if (!flight) continue
    const progress = animation.effect?.getComputedTiming().progress
    if (typeof progress === 'number') {
      dy += flight.dy * (1 - progress)
      scale *= flight.scale + (1 - flight.scale) * progress
    }
    animation.cancel()
  }
  return { dy, scale }
}

/**
 * 字幕区变高、往上让位时，带 `data-flip` 的元素从原来的位置滑到新落点，不跳；
 * 字号变了的那句（当前句退成旧句）同时从原字号缩过去。
 * 位置按布局算（offsetTop），上一段还没走完时从它此刻所在的地方接着走。
 * 所以这些元素的布局只能在提交时变：字号不能再用 CSS 过渡，否则量到的旧位置会过时。
 * root 必须是定位元素，而且自己不跟着字幕动（舞台的场景区）。尺寸变了只更新位置，不动画。
 */
export function useFlipLayout(
  rootRef: RefObject<HTMLElement | null>,
  trigger: unknown,
  enabled: boolean,
) {
  const last = useRef(new Map<string, Placement>())

  const measure = useCallback(
    (animate: boolean) => {
      const root = rootRef.current
      if (!root) return
      const next = new Map<string, Placement>()
      for (const element of root.querySelectorAll<HTMLElement>('[data-flip]')) {
        const id = element.dataset.flip
        if (!id) continue
        const placement = {
          top: layoutTop(element, root),
          font: Number.parseFloat(getComputedStyle(element).fontSize) || 16,
        }
        next.set(id, placement)
        const before = last.current.get(id)
        if (!animate || !before || typeof element.animate !== 'function') continue
        const now = takeFlight(element)
        const dy = before.top + now.dy - placement.top
        const scale = (before.font * now.scale) / placement.font
        if (Math.abs(dy) < 0.5 && Math.abs(scale - 1) < 0.005) continue
        const animation = element.animate(
          [
            { translate: `0 ${dy}px`, scale: `${scale}` },
            { translate: '0 0', scale: '1' },
          ],
          { duration: FLIP_MS, easing: FLIP_EASE, composite: 'add' },
        )
        flights.set(animation, { dy, scale })
      }
      last.current = next
    },
    [rootRef],
  )

  useLayoutEffect(() => {
    measure(enabled)
  }, [enabled, measure, trigger])

  useEffect(() => {
    const root = rootRef.current
    if (!root || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => measure(false))
    observer.observe(root)
    return () => observer.disconnect()
  }, [measure, rootRef])
}
