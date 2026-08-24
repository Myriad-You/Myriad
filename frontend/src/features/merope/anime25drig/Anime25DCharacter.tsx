import type { PerformanceDirective } from '../../../services/agent/types'
import type { SpeechArticulation } from '../rig/articulation'
import type { GazeSource, GazeTarget } from '../rig/motion'
import type { MeropeRigManifest } from '../rig/types'
import type { MeropeActivity } from '../types'
import type { Anime25DDebugSnapshot, Anime25DDriver } from './player'
import type { Anime25DPlayback } from './types'
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
} from 'react'
import {
  baselineDriverPatch,
  cueDriverPatch,
  cueDurationMs,
  cuePriority,
} from './performanceMotion'
import {
  Anime25DPlayer,
  DEFAULT_FRONT_HAIR_SWAY,
  DEFAULT_REAR_HAIR_SWAY,
  IDENTITY_DRIVER,
} from './player'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
} from './speechDriver'

interface Props {
  activity: MeropeActivity
  fallbackUrl: string
  manifest: MeropeRigManifest
  playback: Anime25DPlayback
  atlasUrl: string
  mood: number
  /** Settings page: ignore live activity/mood so sliders stay in charge. */
  manualControl?: boolean
}

export interface Anime25DCharacterHandle {
  setSpeechActive: (active: boolean) => void
  setAutoSpeech: (active: boolean) => void
  setSpeechEnergy: (energy: number | null) => void
  setSpeechArticulation: (articulation: SpeechArticulation) => void
  setGazeTarget: (target: GazeTarget | null, source?: GazeSource) => void
  playMotionPlan: (performance: PerformanceDirective) => void
  stopMotionPlan: () => void
  captureFrame: () => string | null
  setDriver: (partial: Partial<Anime25DDriver>) => void
  replaceDriver: (driver: Anime25DDriver) => void
  resetDriver: () => void
  getDriver: () => Anime25DDriver | null
  blinkNow: () => void
  debugSnapshot: () => Anime25DDebugSnapshot | null
  setMouse: (x: number, y: number, inside: boolean) => void
}

const Anime25DCharacter = forwardRef<Anime25DCharacterHandle, Props>(
  ({ activity, fallbackUrl, manifest, playback, atlasUrl, mood, manualControl = false }, ref) => {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const playerRef = useRef<Anime25DPlayer | null>(null)
    const readyRef = useRef(false)
    const wrapperRef = useRef<HTMLSpanElement>(null)
    const activityRef = useRef(activity)
    const moodRef = useRef(mood)
    const speechActiveRef = useRef(false)
    const manualRef = useRef(manualControl)
    const baselineRef = useRef<Partial<Anime25DDriver> | null>(null)
    const cueTimersRef = useRef<number[]>([])
    const restoreTimerRef = useRef<number | null>(null)
    const activePriorityRef = useRef(0)
    const activeUntilRef = useRef(0)
    const performanceRevisionRef = useRef(-1)
    const performancePhaseRankRef = useRef(-1)
    activityRef.current = activity
    moodRef.current = mood
    manualRef.current = manualControl || manualRef.current

    const applyDriver = (player: Anime25DPlayer) => {
      if (manualRef.current || manualControl) return
      const currentActivity = activityRef.current
      const smile = Math.max(0, (moodRef.current - 50) / 80)
      player.setTarget({
        angleX: 0,
        angleY: 0,
        angleZ: 0,
        eyeOpenL: 1,
        eyeOpenR: 1,
        eyeX: 0,
        eyeY: 0,
        irisScale: 1,
        brow: 0,
        mouthForm: smile * 0.28,
        body: 0,
        armY: 0,
        armPos: 0,
        bust: 2.5,
        physAmp: DEFAULT_REAR_HAIR_SWAY,
        soft: 2,
        fhAmp: DEFAULT_FRONT_HAIR_SWAY,
        idle: true,
        blink: true,
        rand: true,
        phys: true,
        ...(baselineRef.current || {
          mouthForm: smile * 0.28,
        }),
        talk: false,
        mouthOpen: 0,
        ...(currentActivity === 'thinking'
          ? { angleY: 0.08, body: 0.4 }
          : {}),
      })
    }

    const clearCueTimers = () => {
      for (const timer of cueTimersRef.current) window.clearTimeout(timer)
      cueTimersRef.current = []
      if (restoreTimerRef.current !== null) {
        window.clearTimeout(restoreTimerRef.current)
        restoreTimerRef.current = null
      }
      activePriorityRef.current = 0
      activeUntilRef.current = 0
    }

    useImperativeHandle(ref, () => ({
      setSpeechActive(active) {
        speechActiveRef.current = active
        playerRef.current?.setSpeechActive(active)
      },
      setAutoSpeech(active) {
        playerRef.current?.setTarget({
          talk: active,
          mouthOpen: 0,
          mouthForm: baselineRef.current?.mouthForm ?? 0,
        })
      },
      setSpeechEnergy(energy) {
        playerRef.current?.setTarget(speechEnergyDriverPatch(energy))
      },
      setSpeechArticulation(articulation) {
        playerRef.current?.setTarget(
          speechArticulationDriverPatch(
            articulation,
            baselineRef.current?.mouthForm ?? 0,
          ),
        )
      },
      setGazeTarget(target) {
        playerRef.current?.setTarget({
          angleX: target ? target.x * 0.35 : 0,
          angleY: target ? target.y * 0.28 : 0,
        })
      },
      playMotionPlan(directive) {
        if (manualRef.current || manualControl) return
        const phaseRank = {
          mood: 0,
          reaction: 1,
          delivery: 2,
          proactive: 2,
          outcome: 3,
        }[directive.phase]
        if (directive.moodRevision < performanceRevisionRef.current) return
        if (
          directive.moodRevision === performanceRevisionRef.current &&
          phaseRank < performancePhaseRankRef.current
        ) {
          return
        }
        if (directive.moodRevision > performanceRevisionRef.current) {
          clearCueTimers()
          performanceRevisionRef.current = directive.moodRevision
          performancePhaseRankRef.current = -1
        }
        performancePhaseRankRef.current = phaseRank
        if (directive.plan.baseline) {
          baselineRef.current = baselineDriverPatch(directive.plan.baseline)
        }
        if (playerRef.current) applyDriver(playerRef.current)

        for (const cue of directive.plan.cues) {
          const timer = window.setTimeout(() => {
            const run = () => {
              const priority = cuePriority(cue)
              const now = performance.now()
              if (cue.interrupt === 'queue' && now < activeUntilRef.current) {
                const queued = window.setTimeout(run, activeUntilRef.current - now)
                cueTimersRef.current.push(queued)
                return
              }
              if (cue.interrupt === 'if-lower' && priority <= activePriorityRef.current) return
              if (restoreTimerRef.current !== null) window.clearTimeout(restoreTimerRef.current)
              const duration = cueDurationMs(cue)
              activePriorityRef.current = priority
              activeUntilRef.current = performance.now() + duration
              playerRef.current?.setTarget({
                ...(baselineRef.current || {}),
                // Authored cues own the pose until their restore timer fires.
                // Ambient motion eases to neutral instead of competing.
                rand: false,
                ...cueDriverPatch(cue),
              })
              restoreTimerRef.current = window.setTimeout(() => {
                activePriorityRef.current = 0
                activeUntilRef.current = 0
                restoreTimerRef.current = null
                if (playerRef.current) applyDriver(playerRef.current)
              }, duration)
            }
            run()
          }, cue.atMs)
          cueTimersRef.current.push(timer)
        }
      },
      stopMotionPlan() {
        clearCueTimers()
        baselineRef.current = null
        performanceRevisionRef.current = -1
        performancePhaseRankRef.current = -1
        if (playerRef.current) applyDriver(playerRef.current)
      },
      captureFrame() {
        return playerRef.current?.captureFrame() ?? null
      },
      setDriver(partial) {
        manualRef.current = true
        playerRef.current?.setTarget(partial)
      },
      replaceDriver(driver) {
        manualRef.current = true
        playerRef.current?.replaceTarget(driver)
      },
      resetDriver() {
        clearCueTimers()
        baselineRef.current = null
        performanceRevisionRef.current = -1
        performancePhaseRankRef.current = -1
        manualRef.current = false
        playerRef.current?.replaceTarget({ ...IDENTITY_DRIVER })
        if (playerRef.current) applyDriver(playerRef.current)
      },
      getDriver() {
        return playerRef.current?.getTarget() ?? null
      },
      blinkNow() {
        playerRef.current?.blinkNow()
      },
      debugSnapshot() {
        return playerRef.current?.debugSnapshot() ?? null
      },
      setMouse(x, y, inside) {
        playerRef.current?.setMouse(x, y, inside)
      },
    }))

    useEffect(() => {
      const canvas = canvasRef.current
      const wrapper = wrapperRef.current
      if (!canvas || !wrapper) return undefined
      const player = new Anime25DPlayer(canvas, playback, manifest)
      playerRef.current = player
      player.setSpeechActive(speechActiveRef.current)
      applyDriver(player)
      let frame = 0
      let last = performance.now()
      let cancelled = false
      const onPointerMove = (event: PointerEvent) => {
        const bounds = canvas.getBoundingClientRect()
        if (bounds.width <= 0 || bounds.height <= 0) return
        player.setMouse(
          ((event.clientX - bounds.left) / bounds.width) * 2 - 1,
          ((event.clientY - bounds.top) / bounds.height) * 2 - 1,
          true,
        )
      }
      const onPointerLeave = () => {
        player.setMouse(0, 0, false)
      }
      canvas.addEventListener('pointermove', onPointerMove)
      canvas.addEventListener('pointerleave', onPointerLeave)
      const resize = () => {
        const rect = wrapper.getBoundingClientRect()
        player.resize(rect.width, rect.height, window.devicePixelRatio || 1)
      }
      const tick = (now: number) => {
        if (cancelled) return
        player.tick((now - last) / 1000)
        last = now
        frame = window.requestAnimationFrame(tick)
      }
      const observer = new ResizeObserver(resize)
      observer.observe(wrapper)
      resize()
      void player.loadAtlas(atlasUrl).then(() => {
        if (cancelled) return
        readyRef.current = true
        wrapper.classList.add('is-ready')
        frame = window.requestAnimationFrame(tick)
      })
      return () => {
        cancelled = true
        clearCueTimers()
        window.cancelAnimationFrame(frame)
        observer.disconnect()
        canvas.removeEventListener('pointermove', onPointerMove)
        canvas.removeEventListener('pointerleave', onPointerLeave)
        player.dispose()
        playerRef.current = null
        wrapper.classList.remove('is-ready')
      }
    }, [atlasUrl, manifest, playback])

    useEffect(() => {
      if (manualRef.current || manualControl) return
      if (playerRef.current) applyDriver(playerRef.current)
    }, [activity, mood, manualControl])

    return (
      <span
        ref={wrapperRef}
        className="merope-rig"
        data-rig-quality="layered-2d"
        data-runtime="Anime2.5DRig"
      >
        <img src={fallbackUrl} alt="" draggable={false} />
        <canvas ref={canvasRef} aria-hidden />
      </span>
    )
  },
)

export default Anime25DCharacter
