import type { RigBearing } from '../motion/bearing'
import type { BehaviorPlan } from '../motion/behavior'
import type { MotionChannelPolicy } from '../motion/policy'
import type { RigMotionPort } from '../rig/motionPort'
import type { MeropeRigManifest } from '../rig/types'
import type { SingingSpectrumDrive } from '../singing/singingGroove'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { MeropeActivity } from '../types'
import type { Anime25DPlayback } from './types'
import type { Anime25DWorkbenchPort } from './workbenchPort'
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { activityExpressionDriverPatch } from './expressionPresets'
import { idleSpeechDriverPatch } from './performanceMotion'
import { Anime25DPlayer } from './player'
import { shouldAnimateAnime25D } from './runtimePolicy'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
  updatedSpeechMouthFormBaseline,
} from './speechDriver'

interface Props {
  activity: MeropeActivity
  manifest: MeropeRigManifest
  playback: Anime25DPlayback
  atlasUrl: string
  mood: number
  /** Settings page: sliders own the base pose; live acting stays additive. */
  manualControl?: boolean
  onPlaybackError?: (error: unknown) => void
}

export interface Anime25DCharacterHandle
  extends RigMotionPort, Anime25DWorkbenchPort {}

const Anime25DCharacter = forwardRef<Anime25DCharacterHandle, Props>(
  (
    {
      activity,
      manifest,
      playback,
      atlasUrl,
      mood,
      manualControl = false,
      onPlaybackError,
    },
    ref,
  ) => {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const playerRef = useRef<Anime25DPlayer | null>(null)
    const readyRef = useRef(false)
    const [ready, setReady] = useState(false)
    const wrapperRef = useRef<HTMLSpanElement>(null)
    const activityRef = useRef(activity)
    const moodRef = useRef(mood)
    const speechActiveRef = useRef(false)
    const speechProsodyRef = useRef<SpeechProsodyPlan | null>(null)
    const singingActiveRef = useRef(false)
    const singingTrackRef = useRef<string | null>(null)
    const singingSpectrumRef = useRef<SingingSpectrumDrive | null>(null)
    const motionPolicyRef = useRef<MotionChannelPolicy | null>(null)
    const speechMouthFormRef = useRef(0)
    const pendingSpeechTextRef = useRef<
      Array<{ text: string; locale?: string }>
    >([])
    const manualRef = useRef(manualControl)
    const bearingRef = useRef<RigBearing | null>(null)
    const behaviorPlanRef = useRef<BehaviorPlan | null>(null)
    manualRef.current = manualControl || manualRef.current

    const applyDriver = (player: Anime25DPlayer) => {
      if (manualRef.current || manualControl) return
      const currentActivity = activityRef.current
      const policy = player.getMotionPolicy()
      const thinking = currentActivity === 'thinking'
      const expressionFree =
        policy.expression === 'idle' || policy.expression === 'mood'
      const mouthFree = policy.mouth === 'idle'
      player.setTarget({
        thinking,
        blink: true,
        ...(expressionFree ? activityExpressionDriverPatch(thinking) : {}),
        ...(mouthFree
          ? idleSpeechDriverPatch(moodRef.current, speechActiveRef.current)
          : {}),
      })
      if (bearingRef.current) player.setBearing(bearingRef.current)
    }

    const enterManualControl = () => {
      if (manualRef.current) return
      manualRef.current = true
    }

    useImperativeHandle(ref, () => ({
      setBearing(bearing) {
        bearingRef.current = bearing
        playerRef.current?.setBearing(bearing)
      },
      setSpeechActive(active) {
        if (active && !speechActiveRef.current) {
          speechMouthFormRef.current =
            playerRef.current?.getTarget().mouthForm ?? 0
        }
        speechActiveRef.current = active
        playerRef.current?.setSpeechActive(active)
      },
      setSinging(active) {
        singingActiveRef.current = active
        playerRef.current?.setSinging(active)
      },
      setSingingTrack(trackId) {
        singingTrackRef.current = trackId
        playerRef.current?.setSingingTrack(trackId)
      },
      setSingingSpectrum(drive) {
        singingSpectrumRef.current = drive
        playerRef.current?.setSingingSpectrum(drive)
      },
      setAutoSpeech(active) {
        if (!active) playerRef.current?.clearSpeechText()
        playerRef.current?.setTarget({
          talk: active,
          mouthOpen: 0,
          mouthWide: 0,
          mouthRound: 0,
          mouthNarrow: 0,
          mouthSeal: 0,
        })
      },
      setSpeechEnergy(energy) {
        playerRef.current?.setTarget(speechEnergyDriverPatch(energy))
      },
      setSpeechArticulation(articulation) {
        playerRef.current?.setTarget(
          speechArticulationDriverPatch(
            articulation,
            speechMouthFormRef.current,
          ),
        )
      },
      setSpeechProsody(prosody) {
        speechProsodyRef.current = prosody
        playerRef.current?.setSpeechProsody(prosody)
      },
      enqueueSpeechText(text, locale) {
        if (playerRef.current) {
          playerRef.current.enqueueSpeechText(text, locale)
        } else {
          pendingSpeechTextRef.current.push({ text, locale })
        }
      },
      playBehaviorPlan(plan) {
        const now = performance.now()
        const realization = realizeAnime25DBehaviorPlan(plan, now)
        // Restating the whole live set is the entire protocol. The player
        // reconciles it, so nothing here tracks what has already played.
        playerRef.current?.setBehaviorMotionUnits(realization.units, now)
        behaviorPlanRef.current = plan
        return realization.reports
      },
      stopBehaviorPlan(planId) {
        if (planId && behaviorPlanRef.current?.id !== planId) return
        playerRef.current?.clearBehaviorMotionUnits()
        behaviorPlanRef.current = null
      },
      setDriver(partial) {
        enterManualControl()
        speechMouthFormRef.current = updatedSpeechMouthFormBaseline(
          speechMouthFormRef.current,
          speechActiveRef.current,
          partial.mouthForm,
        )
        playerRef.current?.setTarget(partial)
      },
      replaceDriver(driver) {
        enterManualControl()
        speechMouthFormRef.current = updatedSpeechMouthFormBaseline(
          speechMouthFormRef.current,
          speechActiveRef.current,
          driver.mouthForm,
        )
        playerRef.current?.replaceTarget(driver)
      },
      blinkNow() {
        playerRef.current?.blinkNow()
      },
      debugSnapshot() {
        return playerRef.current?.debugSnapshot() ?? null
      },
      setMotionPolicy(policy) {
        motionPolicyRef.current = policy
        playerRef.current?.setMotionPolicy(policy)
      },
      setMood(nextMood, nextActivity) {
        moodRef.current = nextMood
        activityRef.current = nextActivity
        if (playerRef.current) applyDriver(playerRef.current)
      },
    }))

    useEffect(() => {
      const canvas = canvasRef.current
      const wrapper = wrapperRef.current
      if (!canvas || !wrapper) return undefined
      let player: Anime25DPlayer
      try {
        player = new Anime25DPlayer(canvas, playback, manifest)
      } catch (error) {
        readyRef.current = false
        setReady(false)
        onPlaybackError?.(error)
        return undefined
      }
      playerRef.current = player
      player.setSpeechActive(speechActiveRef.current)
      player.setSpeechProsody(speechProsodyRef.current)
      player.setSinging(singingActiveRef.current)
      player.setSingingTrack(singingTrackRef.current)
      player.setSingingSpectrum(singingSpectrumRef.current)
      if (motionPolicyRef.current)
        player.setMotionPolicy(motionPolicyRef.current)
      for (const chunk of pendingSpeechTextRef.current) {
        player.enqueueSpeechText(chunk.text, chunk.locale)
      }
      pendingSpeechTextRef.current = []
      player.setBearing(bearingRef.current)
      if (behaviorPlanRef.current) {
        const now = performance.now()
        const realization = realizeAnime25DBehaviorPlan(
          behaviorPlanRef.current,
          now,
        )
        player.setBehaviorMotionUnits(realization.units, now)
      }
      applyDriver(player)
      let frame = 0
      let last = performance.now()
      let cancelled = false
      let atlasReady = false
      let pageVisible = document.visibilityState !== 'hidden'
      let inViewport = true
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
        frame = 0
        if (cancelled || !atlasReady || !pageVisible || !inViewport) return
        player.tick((now - last) / 1000)
        last = now
        frame = window.requestAnimationFrame(tick)
      }
      const syncAnimation = () => {
        const shouldRun = shouldAnimateAnime25D({
          atlasReady,
          pageVisible,
          inViewport,
          cancelled,
        })
        if (!shouldRun) {
          if (frame !== 0) window.cancelAnimationFrame(frame)
          frame = 0
          return
        }
        if (frame !== 0) return
        last = performance.now()
        frame = window.requestAnimationFrame(tick)
      }
      const onVisibilityChange = () => {
        pageVisible = document.visibilityState !== 'hidden'
        syncAnimation()
      }
      const observer = new ResizeObserver(resize)
      observer.observe(wrapper)
      const viewportObserver =
        typeof IntersectionObserver === 'undefined'
          ? null
          : new IntersectionObserver((entries) => {
              inViewport = entries.some((entry) => entry.isIntersecting)
              syncAnimation()
            })
      viewportObserver?.observe(wrapper)
      document.addEventListener('visibilitychange', onVisibilityChange)
      resize()
      void player
        .loadAtlas(atlasUrl)
        .then(() => {
          if (cancelled) return
          atlasReady = true
          readyRef.current = true
          setReady(true)
          syncAnimation()
        })
        .catch((error: unknown) => {
          if (cancelled) return
          atlasReady = false
          readyRef.current = false
          setReady(false)
          onPlaybackError?.(error)
        })
      return () => {
        cancelled = true
        window.cancelAnimationFrame(frame)
        observer.disconnect()
        viewportObserver?.disconnect()
        document.removeEventListener('visibilitychange', onVisibilityChange)
        canvas.removeEventListener('pointermove', onPointerMove)
        canvas.removeEventListener('pointerleave', onPointerLeave)
        player.dispose()
        playerRef.current = null
        readyRef.current = false
        setReady(false)
      }
    }, [atlasUrl, manifest, onPlaybackError, playback])

    return (
      <span
        ref={wrapperRef}
        className={ready ? 'merope-rig is-ready' : 'merope-rig'}
        data-rig-quality="layered-2d"
        data-runtime="Anime2.5DRig"
      >
        <canvas ref={canvasRef} aria-hidden />
      </span>
    )
  },
)

export default Anime25DCharacter
