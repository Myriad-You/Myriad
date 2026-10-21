import type { RefObject } from 'react'
import type { StageBeat, StageFact } from './stageScript'
import { useEffect, useMemo } from 'react'

/** 当前这句一段一段浮出来，整句最多花这么久，长句每段更快。 */
const REVEAL_TOTAL_MS = 720
const REVEAL_STEP_MAX_MS = 55

const wordSegmenter =
  typeof Intl !== 'undefined' && 'Segmenter' in Intl
    ? new Intl.Segmenter(undefined, { granularity: 'word' })
    : null

/**
 * 按词切：中日文切成词，西文按单词。每段是一个 inline-block，才能上浮；
 * 按字切的话西文单词会在中间断行。空白原样留成文字，照常在那里换行。
 */
function segmentsOf(text: string): { text: string; space: boolean }[] {
  if (wordSegmenter) {
    return Array.from(wordSegmenter.segment(text), (part) => ({
      text: part.segment,
      space: /^\s+$/.test(part.segment),
    }))
  }
  return text
    .split(/(\s+)/)
    .filter(Boolean)
    .map((part) => ({ text: part, space: /^\s+$/.test(part) }))
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

export interface PlacedFact {
  fact: StageFact
  /** 卡边的哪个位置。同时漂着的几条不会落在同一处。 */
  slot: number
}

const SLOTS = 3

/**
 * 这一幕到目前为止提到过的事实，同一条只留最近一次，最多几条，最新的在最后。
 * 位置按在这一幕里第一次出现的先后轮流给；跟还漂着的撞了就往后挪到空位。
 */
export function recentFacts(beats: readonly StageBeat[], shown: number, limit: number): PlacedFact[] {
  const firstSeen = new Map<string, number>()
  const recent = new Map<string, StageFact>()
  for (const beat of beats.slice(0, shown)) {
    if (!beat.fact) continue
    if (!firstSeen.has(beat.fact.id)) firstSeen.set(beat.fact.id, firstSeen.size)
    recent.delete(beat.fact.id)
    recent.set(beat.fact.id, beat.fact)
  }
  const visible = Array.from(recent.values()).slice(-Math.min(limit, SLOTS))
  const taken = new Set<number>()
  return visible.map((fact) => {
    let slot = (firstSeen.get(fact.id) ?? 0) % SLOTS
    while (taken.has(slot)) slot = (slot + 1) % SLOTS
    taken.add(slot)
    return { fact, slot }
  })
}

/** 事实是卡片边上冒出来的气泡：新的弹出来，旧的退淡、变小。 */
export function FactBubbles({ facts }: { facts: readonly PlacedFact[] }) {
  return (
    <>
      {facts.map(({ fact, slot }, index) => (
        <div
          key={fact.id}
          className="report-stage__bubble"
          data-slot={slot}
          data-age={facts.length - 1 - index}
        >
          <div className="report-stage__bubble-body">
            {fact.media ? (
              <img
                src={fact.media}
                alt=""
                loading="lazy"
                decoding="async"
                // 封面、头像取不到时不留破图标。
                onError={(event) => event.currentTarget.remove()}
              />
            ) : null}
            <span>
              <small>{fact.label}</small>
              <strong>{fact.value}</strong>
            </span>
          </div>
        </div>
      ))}
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

/**
 * 卡片跟着指针微微侧过去。直接写 CSS 变量，不走 React 状态；一帧最多算一次。
 * 触屏、关了循环动效时不跟。
 */
export function usePointerTilt(
  rootRef: RefObject<HTMLElement | null>,
  targetRef: RefObject<HTMLElement | null>,
  enabled: boolean,
) {
  useEffect(() => {
    const root = rootRef.current
    if (!enabled || !root || !matchMedia('(pointer: fine)').matches) return
    let frame = 0
    let x = 0
    let y = 0
    const apply = () => {
      frame = 0
      const target = targetRef.current
      if (!target) return
      target.style.setProperty('--tilt-x', `${(-y * 6).toFixed(2)}deg`)
      target.style.setProperty('--tilt-y', `${(x * 9).toFixed(2)}deg`)
    }
    const onMove = (event: PointerEvent) => {
      const rect = root.getBoundingClientRect()
      x = (event.clientX - rect.left) / Math.max(1, rect.width) - 0.5
      y = (event.clientY - rect.top) / Math.max(1, rect.height) - 0.5
      frame ||= requestAnimationFrame(apply)
    }
    const onLeave = () => {
      x = 0
      y = 0
      frame ||= requestAnimationFrame(apply)
    }
    root.addEventListener('pointermove', onMove)
    root.addEventListener('pointerleave', onLeave)
    return () => {
      root.removeEventListener('pointermove', onMove)
      root.removeEventListener('pointerleave', onLeave)
      if (frame) cancelAnimationFrame(frame)
      targetRef.current?.style.removeProperty('--tilt-x')
      targetRef.current?.style.removeProperty('--tilt-y')
    }
  }, [enabled, rootRef, targetRef])
}
