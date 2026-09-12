/** 不碰订阅轨 flip，换树只改行属性。 */

import type { CSSProperties, ReactNode } from 'react'
import type { ChipPhase } from '../../../hooks/animation/pages/brewChipPresence'

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import {
  BREW_TAG_ENTER_MS,
  BREW_TAG_EXIT_MS,
  brewMotionClaim,
  brewMotionRelease,
  brewTagDelay,
  brewTagQuiet,
  useBrewTag,
} from '../../../hooks/animation/pages/brew'
import {
  awaitLaneSwap,

  planChipLaneSwap,
  playBrewChipEnter,
  playBrewChipExit,
  shouldPlayChipEnter,
} from '../../../hooks/animation/pages/brewChipPresence'
import { cx } from './cx'

const ChipLaneMotion = createContext(false)

export function BrewChip({
  id,
  index,
  phase = 'enter',
  children,
  grow,
  conceal,
}: {
  id: string
  index: number
  phase?: ChipPhase
  children: ReactNode
  grow?: boolean
  conceal?: boolean
}) {
  const quiet = brewTagQuiet()
  const lanePlays = useContext(ChipLaneMotion)
  const role = quiet || lanePlays ? 'in' : phase
  const { onComplete } = useBrewTag(index, {
    enabled: !quiet && !lanePlays,
    role,
    chipId: id,
  })
  const bornRef = useRef(
    typeof performance === 'undefined' ? 0 : performance.now(),
  )
  const [settled, setSettled] = useState(quiet || role === 'in')

  useEffect(() => {
    if (quiet || role !== 'enter' || settled) return
    const timer = window.setTimeout(setSettled, brewTagDelay(index) + BREW_TAG_ENTER_MS + 80, true)
    return () => window.clearTimeout(timer)
  }, [quiet, role, settled, index])

  const motion = role === 'enter' && !settled ? 'is-enter' : ''

  return (
    <div
      className={cx(
        'brew-bar__chip',
        grow && 'is-grow',
        motion,
        conceal && 'is-conceal',
      )}
      style={{ '--brew-tag-i': index } as CSSProperties}
      data-brew-chip={id}
      aria-hidden={conceal || undefined}
      inert={conceal || undefined}
      onAnimationEnd={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.animationName !== 'brew-tag-enter') return
        if (performance.now() - bornRef.current < BREW_TAG_ENTER_MS * 0.8) {
          return
        }
        setSettled(true)
        onComplete()
      }}
    >
      {children}
    </div>
  )
}

export function useBrewWaveLane(
  wave: string,
  children: ReactNode,
  play: (node: HTMLElement | null) => {
    wait: number
    waapi: boolean
    done: Promise<void>
  },
  onDisplayed?: (wave: string) => void,
  occupy?: boolean,
) {
  const rowRef = useRef<HTMLDivElement>(null)
  const oldChildrenRef = useRef(children)
  const incomingRef = useRef(children)
  const pendingWaveRef = useRef(wave)
  const exitingRef = useRef(false)
  const shownRef = useRef(wave)
  incomingRef.current = children

  const [shown, setShown] = useState(wave)
  const [exiting, setExiting] = useState(false)
  const [exitHow, setExitHow] = useState<'waapi' | 'css'>('css')
  const [arriving, setArriving] = useState(() => !brewTagQuiet())
  shownRef.current = shown
  const onDisplayedRef = useRef(onDisplayed)
  onDisplayedRef.current = onDisplayed
  const clearArriving = useCallback(() => setArriving(false), [])

  if (!exiting && wave === shown) {
    oldChildrenRef.current = children
  }

  useLayoutEffect(() => {
    onDisplayedRef.current?.(shown)
  }, [shown])

  useLayoutEffect(() => {
    const plan = planChipLaneSwap(shownRef.current, wave, exitingRef.current)
    if (plan === 'hold') {
      oldChildrenRef.current = incomingRef.current
      return
    }
    pendingWaveRef.current = wave
    if (plan === 'retarget') return

    if (brewTagQuiet()) {
      shownRef.current = wave
      oldChildrenRef.current = incomingRef.current
      exitingRef.current = false
      setShown(wave)
      setExiting(false)
      setArriving(false)
      return
    }

    const token = occupy ? brewMotionClaim('lane') : null
    const { wait, waapi, done } = play(rowRef.current)
    exitingRef.current = true
    setExitHow(waapi ? 'waapi' : 'css')
    setExiting(true)

    const finish = () => {
      if (!exitingRef.current) return
      exitingRef.current = false
      const next = pendingWaveRef.current
      shownRef.current = next
      oldChildrenRef.current = incomingRef.current
      setShown(next)
      setExiting(false)
      if (token != null) brewMotionRelease(token)
    }

    void awaitLaneSwap(done, wait).then(finish)
    return () => {
      if (exitingRef.current) return
      if (token != null) brewMotionRelease(token)
    }
  }, [wave, play, occupy])

  const frozen = exiting || wave !== shown
  return {
    rowRef,
    frozen,
    exiting,
    exitHow,
    shown,
    arriving,
    clearArriving,
    view: frozen ? oldChildrenRef.current : children,
  }
}

export function BrewChipLane({
  children,
  wave,
  onDisplayed,
}: {
  children: ReactNode
  wave: string
  onDisplayed?: (wave: string) => void
}) {
  const outRef = useRef<HTMLDivElement>(null)
  const inRef = useRef<HTMLDivElement>(null)
  const play = useCallback(
    (node: HTMLElement | null) => playBrewChipExit(outRef.current ?? node),
    [],
  )
  const {
    rowRef,
    frozen,
    exiting,
    exitHow,
    shown,
    arriving,
    clearArriving,
    view,
  } = useBrewWaveLane(wave, children, play, onDisplayed, false)

  useLayoutEffect(() => {
    if (!shouldPlayChipEnter(frozen)) return
    playBrewChipEnter(inRef.current)
    clearArriving()
  }, [frozen, shown, clearArriving])

  const dest = frozen ? children : view
  const hideIn = arriving || frozen
  const lanePlays = frozen || arriving

  return (
    <ChipLaneMotion.Provider value={lanePlays}>
      <div
        className="brew-bar__row"
        ref={rowRef}
        data-chip-phase={exiting ? 'exit' : arriving ? 'enter' : 'in'}
        data-chip-exit={exiting ? exitHow : undefined}
        data-brew-wave={shown}
      >
        {frozen ? (
          <div
            ref={outRef}
            className="brew-bar__set is-leave"
            aria-hidden
          >
            {view}
          </div>
        ) : null}
        <div
          ref={inRef}
          className={cx('brew-bar__set', hideIn && 'is-arrive')}
          key={wave}
          aria-hidden={hideIn}
          inert={hideIn || undefined}
        >
          {dest}
        </div>
      </div>
    </ChipLaneMotion.Provider>
  )
}

export function BrewPanelLane({ children }: { children?: ReactNode }) {
  const has = children != null && children !== false
  const pendingRef = useRef(children)
  const panelRef = useRef<HTMLDivElement>(null)
  pendingRef.current = children
  const [view, setView] = useState<{
    node: ReactNode
    phase: ChipPhase
  } | null>(has ? { node: children, phase: 'enter' } : null)

  useLayoutEffect(() => {
    if (brewTagQuiet()) {
      setView(has ? { node: children, phase: 'in' } : null)
      return
    }

    if (has) {
      setView((prev) => ({
        node: children,
        phase: prev ? 'in' : 'enter',
      }))
      return
    }

    setView((prev) => (prev ? { ...prev, phase: 'exit' } : null))
    const panel = panelRef.current
    if (panel && typeof panel.animate === 'function') {
      const style = getComputedStyle(panel)
      for (const anim of panel.getAnimations()) anim.cancel()
      panel.animate(
        [
          { opacity: style.opacity, transform: style.transform },
          {
            opacity: 0,
            transform: 'translate3d(0, var(--sm-shift-sm, 6px), 0) scale(0.96)',
          },
        ],
        {
          duration: BREW_TAG_EXIT_MS,
          easing: 'cubic-bezier(0.4, 0, 1, 1)',
          fill: 'forwards',
        },
      )
    }
    const timer = window.setTimeout(() => {
      if (!pendingRef.current) setView(null)
    }, BREW_TAG_EXIT_MS + 32)
    return () => window.clearTimeout(timer)
  }, [has, children])

  if (!view) return null
  return (
    <div
      ref={panelRef}
      className={cx(
        'brew-bar__panel',
        view.phase === 'enter' && 'is-enter',
        view.phase === 'exit' && 'is-exit',
      )}
    >
      {view.node}
    </div>
  )
}
