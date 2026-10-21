import { useMemo } from 'react'

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
