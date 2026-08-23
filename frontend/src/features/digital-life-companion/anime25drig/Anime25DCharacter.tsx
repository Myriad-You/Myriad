import type { CompanionActivity } from '../types'
import type { SpeechArticulation } from '../rig/articulation'
import type { GazeSource, GazeTarget } from '../rig/motion'
import type { Anime25DPlayback } from './types'
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
} from 'react'
import {
  Anime25DPlayer,
  IDENTITY_DRIVER,
  type Anime25DDebugSnapshot,
  type Anime25DDriver,
} from './player'

interface Props {
  activity: CompanionActivity
  fallbackUrl: string
  playback: Anime25DPlayback
  atlasUrl: string
  mood: number
  /** Settings page: ignore live activity/mood so sliders stay in charge. */
  manualControl?: boolean
}

export interface Anime25DCharacterHandle {
  setSpeechEnergy: (energy: number | null) => void
  setSpeechArticulation: (articulation: SpeechArticulation) => void
  setGazeTarget: (target: GazeTarget | null, source?: GazeSource) => void
  playMotionPlan: () => void
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
  ({ activity, fallbackUrl, playback, atlasUrl, mood, manualControl = false }, ref) => {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const playerRef = useRef<Anime25DPlayer | null>(null)
    const readyRef = useRef(false)
    const wrapperRef = useRef<HTMLSpanElement>(null)
    const activityRef = useRef(activity)
    const moodRef = useRef(mood)
    const manualRef = useRef(manualControl)
    activityRef.current = activity
    moodRef.current = mood
    manualRef.current = manualControl || manualRef.current

    const applyDriver = (player: Anime25DPlayer) => {
      if (manualRef.current || manualControl) return
      const currentActivity = activityRef.current
      const smile = Math.max(0, (moodRef.current - 50) / 80)
      player.setTarget({
        talk: currentActivity === 'talking',
        mouthOpen: currentActivity === 'talking' ? 0.42 : smile * 0.12,
        angleY: currentActivity === 'thinking' ? 0.08 : 0,
        body: currentActivity === 'thinking' ? 0.4 : 0,
      })
    }

    useImperativeHandle(ref, () => ({
      setSpeechEnergy(energy) {
        playerRef.current?.setTarget({
          mouthOpen: energy == null ? 0 : Math.max(0, Math.min(1, energy)),
          talk: energy != null && energy > 0.08,
        })
      },
      setSpeechArticulation(articulation) {
        const openness =
          articulation.viseme === 'closed' || articulation.viseme === 'rest'
            ? 0.08
            : articulation.viseme === 'wide'
              ? 0.92
              : 0.55
        playerRef.current?.setTarget({ mouthOpen: openness, talk: true })
      },
      setGazeTarget(target) {
        playerRef.current?.setTarget({
          angleX: target ? target.x * 0.35 : 0,
          angleY: target ? target.y * 0.28 : 0,
        })
      },
      playMotionPlan() {
        manualRef.current = true
        playerRef.current?.setTarget({
          talk: true,
          mouthOpen: 0.45,
          angleY: -0.08,
          bust: 2.5,
        })
      },
      stopMotionPlan() {
        playerRef.current?.setTarget({
          mouthOpen: 0,
          talk: false,
          armY: 0,
          armPos: 0,
        })
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
      const player = new Anime25DPlayer(canvas, playback)
      playerRef.current = player
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
        window.cancelAnimationFrame(frame)
        observer.disconnect()
        canvas.removeEventListener('pointermove', onPointerMove)
        canvas.removeEventListener('pointerleave', onPointerLeave)
        player.dispose()
        playerRef.current = null
        wrapper.classList.remove('is-ready')
      }
    }, [atlasUrl, playback])

    useEffect(() => {
      if (manualRef.current || manualControl) return
      const smile = Math.max(0, (mood - 50) / 80)
      playerRef.current?.setTarget({
        talk: activity === 'talking',
        mouthOpen: activity === 'talking' ? 0.42 : smile * 0.12,
        angleY: activity === 'thinking' ? 0.08 : 0,
        body: activity === 'thinking' ? 0.4 : 0,
      })
    }, [activity, mood, manualControl])

    return (
      <span
        ref={wrapperRef}
        className="dlc-rig"
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
