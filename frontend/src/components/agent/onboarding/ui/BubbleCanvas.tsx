import type { CSSProperties, PointerEvent as ReactPointerEvent } from 'react'
import { LuCheck } from '@lib/icons'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

export interface BubbleItem {
  id: string
  label: string
  weight: number
  selected: boolean
  disabled?: boolean
}

interface Placed extends BubbleItem {
  size: number
  x: number
  y: number
  tint: number
  hue: number
  kin: number
  shift: number
  bright: number
}

interface Layout {
  placed: Placed[]
  width: number
  height: number
}

const EMPTY_LAYOUT: Layout = { placed: [], width: 0, height: 0 }

function mulberry32(seed: number): () => number {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6D2B79F5) | 0
    let t = Math.imul(state ^ (state >>> 15), 1 | state)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function hashOf(seed: string): number {
  let hash = 2166136261
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i)
    hash = Math.imul(hash, 16777619)
  }
  return hash >>> 0
}

const WIDE_CHAR = /[\u2E80-\u9FFF\uFF00-\uFFEF]/

function widthUnits(label: string): number {
  let units = 0
  for (const char of label) {
    units += WIDE_CHAR.test(char) ? 2 : 1
  }
  return units
}

function baseSize(item: BubbleItem): number {
  const textFloor = 38 + widthUnits(item.label) * 5.2
  const weighted = 46 + Math.min(Math.max(item.weight, 0), 1) ** 1.4 * 96
  return Math.max(textFloor, weighted)
}

const GAP = 8

const SLACK = 1.32

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high)
}

function overlaps(
  x: number,
  y: number,
  radius: number,
  placed: Placed[],
): boolean {
  for (const other of placed) {
    const gap = Math.hypot(x - other.x, y - other.y) - (radius + other.size / 2)
    if (gap < GAP) return true
  }
  return false
}

function scanFree(
  radius: number,
  placed: Placed[],
  areaW: number,
  areaH: number,
): { x: number; y: number } | null {
  const step = Math.max(8, radius / 2)
  for (let y = radius; y <= areaH - radius; y += step) {
    for (let x = radius; x <= areaW - radius; x += step) {
      if (!overlaps(x, y, radius, placed)) return { x, y }
    }
  }
  return null
}

function pack(items: BubbleItem[], viewW: number, viewH: number): Layout {
  if (items.length === 0 || viewW <= 0 || viewH <= 0) return EMPTY_LAYOUT

  const random = mulberry32(hashOf(items.map((item) => item.id).join('|')))
  const order = items
    .map((item) => ({ item, size: baseSize(item) }))
    .toSorted((a, b) => b.size - a.size)

  const needed = order.reduce((sum, entry) => sum + (entry.size + GAP) ** 2, 0)
  const startScale = Math.max(SLACK, Math.sqrt(needed / (viewW * viewH * 0.55)))
  let areaW = viewW * startScale
  let areaH = viewH * startScale

  const placed: Placed[] = []
  for (const entry of order) {
    const radius = entry.size / 2
    let spot: { x: number; y: number } | null = null

    for (let grow = 0; grow < 8 && !spot; grow += 1) {
      const minX = radius
      const maxX = Math.max(minX, areaW - radius)
      const minY = radius
      const maxY = Math.max(minY, areaH - radius)

      if (placed.length === 0) {
        spot = {
          x: clamp(areaW / 2 + (random() - 0.5) * areaW * 0.1, minX, maxX),
          y: clamp(areaH / 2 + (random() - 0.5) * areaH * 0.1, minY, maxY),
        }
        break
      }

      let best: { x: number; y: number; score: number } | null = null
      for (let attempt = 0; attempt < 160; attempt += 1) {
        const x = minX + random() * (maxX - minX)
        const y = minY + random() * (maxY - minY)
        if (overlaps(x, y, radius, placed)) continue
        let nearest = Infinity
        for (const other of placed) {
          nearest = Math.min(
            nearest,
            Math.hypot(x - other.x, y - other.y) - (radius + other.size / 2),
          )
        }
        const pull =
          Math.hypot((x - areaW / 2) / areaW, (y - areaH / 2) / areaH) *
          (areaW + areaH) *
          0.34
        const score = nearest + pull
        if (!best || score < best.score) best = { x, y, score }
        if (nearest < GAP + 4) break
      }

      spot = best ?? scanFree(radius, placed, areaW, areaH)
      if (!spot) {
        areaW *= 1.18
        areaH *= 1.18
      }
    }

    if (!spot) {
      areaH += entry.size + GAP * 2
      spot = { x: radius, y: areaH - radius }
    }

    const seed = hashOf(entry.item.label || entry.item.id)
    placed.push({
      ...entry.item,
      size: entry.size,
      x: spot.x,
      y: spot.y,
      tint: seed % 101,
      hue: seed % 360,
      kin: 52 + ((seed >>> 17) % 24),
      shift: ((seed >>> 7) % 72) - 36,
      bright: 0.88 + (((seed >>> 13) % 28) / 28) * 0.28,
    })
  }

  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const bubble of placed) {
    minX = Math.min(minX, bubble.x - bubble.size / 2)
    minY = Math.min(minY, bubble.y - bubble.size / 2)
    maxX = Math.max(maxX, bubble.x + bubble.size / 2)
    maxY = Math.max(maxY, bubble.y + bubble.size / 2)
  }
  const contentW = Math.max(1, maxX - minX)
  const contentH = Math.max(1, maxY - minY)
  const width = Math.max(contentW, viewW * SLACK)
  const height = Math.max(contentH, viewH * SLACK)
  const padX = (width - contentW) / 2
  const padY = (height - contentH) / 2
  for (const bubble of placed) {
    bubble.x = bubble.x - minX + padX
    bubble.y = bubble.y - minY + padY
  }

  return { placed, width, height }
}

interface Props {
  label: string
  items: BubbleItem[]
  onToggle: (id: string) => void
  onPannableChange?: (pannable: boolean) => void
}

export default function BubbleCanvas({
  label,
  items,
  onToggle,
  onPannableChange,
}: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const [box, setBox] = useState({ width: 0, height: 0 })
  const [pan, setPan] = useState({ x: 0, y: 0 })
  const drag = useRef<{
    pointer: number
    startX: number
    startY: number
    originX: number
    originY: number
    moved: boolean
  } | null>(null)
  const suppressClick = useRef(false)

  // measure now; some hosts never fire ResizeObserver
  useLayoutEffect(() => {
    const node = ref.current
    if (!node) return undefined
    const measure = () => {
      const { width, height } = node.getBoundingClientRect()
      setBox((current) =>
        Math.abs(current.width - width) < 1 &&
        Math.abs(current.height - height) < 1
          ? current
          : { width, height },
      )
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(node)
    return () => observer.disconnect()
  }, [])

  // selection is not a layout input
  const signature = items.map((item) => `${item.id}:${item.weight}`).join('|')
  const itemsRef = useRef(items)
  itemsRef.current = items
  const layout = useMemo(
    () => pack(itemsRef.current, box.width, box.height),
    [signature, box.width, box.height],
  )

  const liveById = useMemo(
    () => new Map(items.map((item) => [item.id, item])),
    [items],
  )

  const overflowX = Math.max(0, layout.width - box.width)
  const overflowY = Math.max(0, layout.height - box.height)
  const pannable = overflowX > 1 || overflowY > 1

  useEffect(() => {
    onPannableChange?.(pannable)
  }, [onPannableChange, pannable])

  const clampPan = useCallback(
    (x: number, y: number) => ({
      x:
        overflowX > 0
          ? clamp(x, -overflowX, 0)
          : (box.width - layout.width) / 2,
      y:
        overflowY > 0
          ? clamp(y, -overflowY, 0)
          : (box.height - layout.height) / 2,
    }),
    [box.width, box.height, layout.width, layout.height, overflowX, overflowY],
  )

  useEffect(() => {
    setPan((current) => {
      const next = clampPan(current.x, current.y)
      // same object if unchanged; a new identity loops
      return next.x === current.x && next.y === current.y ? current : next
    })
  }, [clampPan])

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pannable || event.button !== 0) return
    if ((event.target as Element | null)?.closest?.('button')) return
    drag.current = {
      pointer: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: pan.x,
      originY: pan.y,
      moved: false,
    }
    try {
      event.currentTarget.setPointerCapture(event.pointerId)
    } catch {
      /* pointermove still tracks */
    }
  }

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const state = drag.current
    if (!state || state.pointer !== event.pointerId) return
    const dx = event.clientX - state.startX
    const dy = event.clientY - state.startY
    if (!state.moved && Math.hypot(dx, dy) > 4) state.moved = true
    if (!state.moved) return
    setPan(clampPan(state.originX + dx, state.originY + dy))
  }

  const endDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    const state = drag.current
    if (!state || state.pointer !== event.pointerId) return
    suppressClick.current = state.moved
    drag.current = null
    try {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId)
      }
    } catch {
      /* already released */
    }
  }

  return (
    <div
      className={`merope-bubble-canvas ${pannable ? 'is-pannable' : ''}`}
      ref={ref}
      role="group"
      aria-label={label}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onClickCapture={(event) => {
        if (!suppressClick.current) return
        suppressClick.current = false
        event.preventDefault()
        event.stopPropagation()
      }}
    >
      <div
        className="merope-bubble-canvas__layer"
        style={{ translate: `${pan.x}px ${pan.y}px` }}
      >
        {layout.placed.map((bubble, index) => {
          const live = liveById.get(bubble.id) ?? bubble
          return (
            <button
              key={bubble.id}
              type="button"
              className={`merope-choice merope-choice--bubble ${live.selected ? 'is-on' : ''}`}
              style={
                {
                  '--choice-i': String(Math.min(index, 24)),
                  '--bubble-size': `${Math.round(bubble.size)}px`,
                  '--bubble-mix': String(bubble.tint),
                  '--bubble-hue': String(bubble.hue),
                  '--bubble-kin': String(bubble.kin),
                  '--bubble-shift': `${bubble.shift}deg`,
                  '--bubble-bright': bubble.bright.toFixed(2),
                  left: `${bubble.x}px`,
                  top: `${bubble.y}px`,
                } as CSSProperties
              }
              aria-pressed={live.selected}
              disabled={live.disabled}
              onClick={(event) => {
                event.stopPropagation()
                if (live.disabled) return
                if (suppressClick.current) {
                  suppressClick.current = false
                  event.preventDefault()
                  return
                }
                onToggle(bubble.id)
              }}
            >
              <span className="merope-choice__tick" aria-hidden>
                <LuCheck />
              </span>
              <span className="merope-choice__label">{live.label}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}
