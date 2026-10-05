import type { PointerEvent as ReactPointerEvent, RefObject } from 'react'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { PoseCorrectionStagePick } from '../anime25drig/workbenchPort'
import type { RigCharacterHandle } from '../character/RigCharacter'
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { clampPoseCorrectionPatch } from '../anime25drig/poseAuthoring'

type Patch = PoseCorrection['patches'][number]
type Grip = 'push' | 'origin' | 'reachX' | 'reachY'

interface Placed {
  /** Where this patch pushes the spot. */
  x: number
  y: number
  /** Where the spot is drawn without this patch. */
  ox: number
  oy: number
  rx: number
  ry: number
  weight: number
}

interface Props {
  characterRef: RefObject<RigCharacterHandle | null>
  correction: PoseCorrection | null
  active: number
  picking: boolean
  /** Points are drawn only while the stage shows the correction at full weight. */
  visible: boolean
  disabled: boolean
  labels: { push: string; origin: string; reach: string; picking: string }
  onPick: (pick: PoseCorrectionStagePick | null) => void
  onCancelPick: () => void
  onSelect: (patch: number) => void
  onChange: (patch: number, next: Patch) => void
}

function samePlaces(a: readonly (Placed | null)[], b: readonly (Placed | null)[]): boolean {
  return a.length === b.length && a.every((p, i) => {
    const q = b[i]
    if (!p || !q) return p === q
    return Math.abs(p.x - q.x) < 0.3 && Math.abs(p.y - q.y) < 0.3 && Math.abs(p.ox - q.ox) < 0.3
      && Math.abs(p.oy - q.oy) < 0.3 && Math.abs(p.rx - q.rx) < 0.3 && Math.abs(p.ry - q.ry) < 0.3
      && Math.abs(p.weight - q.weight) < 0.01
  })
}

/**
 * The correction's points drawn over the live stage. Drag the solid point to
 * where the feature should be, the hollow one to move the corrected area, and
 * the squares to change its reach. While picking, a click on the face or hair
 * names the place to correct.
 */
export function PoseCorrectionOverlay({
  characterRef,
  correction,
  active,
  picking,
  visible,
  disabled,
  labels,
  onPick,
  onCancelPick,
  onSelect,
  onChange,
}: Props) {
  const [places, setPlaces] = useState<(Placed | null)[]>([])
  const placesRef = useRef(places)
  placesRef.current = places
  const [stage, setStage] = useState<DOMRect | null>(null)
  const drag = useRef<{ grip: Grip; patch: number; x: number; y: number } | null>(null)
  const latest = useRef({ correction, onChange })
  latest.current = { correction, onChange }

  useEffect(() => {
    let frame = 0
    const tick = () => {
      const character = characterRef.current
      const current = latest.current.correction
      const next = !character || !current || !visible
        ? []
        : current.patches.map((patch) => {
            const at = character.projectPoseCorrectionPatch(current, patch)
            if (!at) return null
            return {
              x: at.pushedX,
              y: at.pushedY,
              ox: at.originX,
              oy: at.originY,
              rx: patch.radiusX * at.unitX,
              ry: patch.radiusY * at.unitY,
              weight: at.weight,
            }
          })
      setPlaces(previous => samePlaces(previous, next) ? previous : next)
      const rect = character?.poseCorrectionStageRect() ?? null
      setStage(previous =>
        previous && rect && previous.left === rect.left && previous.top === rect.top
          && previous.width === rect.width && previous.height === rect.height
          ? previous
          : rect)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [characterRef, visible])

  useEffect(() => {
    if (!picking) return undefined
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onCancelPick()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancelPick, picking])

  const nudge = (patch: number, grip: Grip, dx: number, dy: number) => {
    const current = latest.current.correction
    const character = characterRef.current
    const source = current?.patches[patch]
    if (!current || !character || !source) return
    const delta = character.poseCorrectionDelta(current.surface, dx, dy)
    if (!delta) return
    const next = { ...source }
    if (grip === 'push') {
      // The push shows at the pose's weight: scale it so the point keeps up.
      const weight = Math.max(0.2, placesRef.current[patch]?.weight ?? 1)
      next.dx += delta.dx / weight
      next.dy += delta.dy / weight
    } else if (grip === 'origin') {
      next.x += delta.dx
      next.y += delta.dy
    } else if (grip === 'reachX') {
      next.radiusX += delta.dx
    } else {
      next.radiusY += delta.dy
    }
    latest.current.onChange(patch, clampPoseCorrectionPatch(next))
  }

  const grab = (patch: number, grip: Grip) => (event: ReactPointerEvent<SVGElement>) => {
    if (disabled || event.button !== 0) return
    event.preventDefault()
    event.stopPropagation()
    onSelect(patch)
    event.currentTarget.setPointerCapture(event.pointerId)
    drag.current = { grip, patch, x: event.clientX, y: event.clientY }
  }
  const move = (event: ReactPointerEvent<SVGElement>) => {
    const current = drag.current
    if (!current) return
    nudge(current.patch, current.grip, event.clientX - current.x, event.clientY - current.y)
    current.x = event.clientX
    current.y = event.clientY
  }
  const release = () => {
    drag.current = null
  }
  const handlers = (patch: number, grip: Grip) => ({
    onPointerDown: grab(patch, grip),
    onPointerMove: move,
    onPointerUp: release,
    onPointerCancel: release,
  })

  const content = (
    <>
      {picking && stage ? (
        <div
          className="merope-pose-overlay__pick"
          style={{ left: stage.left, top: stage.top, width: stage.width, height: stage.height }}
          onClick={event => onPick(characterRef.current?.pickPoseCorrectionPoint(event.clientX, event.clientY) ?? null)}
        >
          <span className="merope-pose-overlay__pick-hint">{labels.picking}</span>
        </div>
      ) : null}
      {visible && !picking && correction && stage ? (
        <svg className="merope-pose-overlay" aria-hidden={false}>
          <defs>
            <clipPath id="merope-pose-overlay-stage">
              <rect x={stage.left} y={stage.top} width={stage.width} height={stage.height} />
            </clipPath>
          </defs>
          <g clipPath="url(#merope-pose-overlay-stage)">
          {places.map((place, index) => {
            if (!place) return null
            const on = index === active
            return (
              <g key={index} className={`merope-pose-overlay__patch${on ? ' is-on' : ''}`}>
                <ellipse cx={place.ox} cy={place.oy} rx={place.rx} ry={place.ry} />
                <line x1={place.ox} y1={place.oy} x2={place.x} y2={place.y} />
                {on ? (
                  <>
                    <rect
                      className="merope-pose-overlay__reach"
                      x={place.ox + place.rx - 5}
                      y={place.oy - 5}
                      width={10}
                      height={10}
                      aria-label={labels.reach}
                      {...handlers(index, 'reachX')}
                    />
                    <rect
                      className="merope-pose-overlay__reach is-y"
                      x={place.ox - 5}
                      y={place.oy + place.ry - 5}
                      width={10}
                      height={10}
                      aria-label={labels.reach}
                      {...handlers(index, 'reachY')}
                    />
                    <circle
                      className="merope-pose-overlay__origin"
                      cx={place.ox}
                      cy={place.oy}
                      r={6}
                      aria-label={labels.origin}
                      {...handlers(index, 'origin')}
                    />
                  </>
                ) : null}
                <circle
                  className="merope-pose-overlay__push"
                  cx={place.x}
                  cy={place.y}
                  r={on ? 8 : 6}
                  tabIndex={on ? 0 : -1}
                  role="button"
                  aria-label={labels.push}
                  onKeyDown={(event) => {
                    const step = event.shiftKey ? 4 : 1
                    const keys: Record<string, [number, number]> = {
                      ArrowLeft: [-step, 0],
                      ArrowRight: [step, 0],
                      ArrowUp: [0, -step],
                      ArrowDown: [0, step],
                    }
                    const delta = keys[event.key]
                    if (!delta || disabled) return
                    event.preventDefault()
                    nudge(index, 'push', delta[0], delta[1])
                  }}
                  {...handlers(index, 'push')}
                />
              </g>
            )
          })}
          </g>
        </svg>
      ) : null}
    </>
  )
  return createPortal(content, document.body)
}
