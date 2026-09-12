import type { RigBearing } from '../motion/bearing'
import type { BehaviorPlan } from '../motion/behavior'
import type { MotionChannelPolicy } from '../motion/policy'
import type { RigMotionPort } from '../rig/motionPort'
import type { MeropeRigManifest } from '../rig/types'
import type { MusicMotionSignal } from '../singing/musicSignal'
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
import { bindCharacterTouch } from '../interaction/bindTouch'
import { createTouchAppraisal } from '../interaction/touchAppraisalHost'
import { getProductionMotionRuntime } from '../motion/runtimeHost'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { activityExpressionDriverPatch } from './expressionPresets'
import { idleSpeechDriverPatch } from './performanceMotion'
import { Anime25DPlayer } from './player'
import {
  intersectionKeepsAnime25DVisible,
  shouldAnimateAnime25D,
} from './runtimePolicy'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
} from './speechDriver'

interface Props {
  activity: MeropeActivity
  manifest: MeropeRigManifest
  playback: Anime25DPlayback
  atlasUrl: string
  mood: number
  manualControl?: boolean
  touchEnabled?: boolean
  onPlaybackError?: (error: unknown) => void
  onPlaybackReady?: () => void
}

const GPU_RECOVERIES = 2

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
      touchEnabled = false,
      onPlaybackError,
      onPlaybackReady,
    },
    ref,
  ) => {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const playerRef = useRef<Anime25DPlayer | null>(null)
    const readyRef = useRef(false)
    const recoveriesRef = useRef(0)
    const [ready, setReady] = useState(false)
    const [gpuEpoch, setGpuEpoch] = useState(0)
    const wrapperRef = useRef<HTMLSpanElement>(null)
    useEffect(() => {
      const canvas = canvasRef.current
      const player = playerRef.current
      if (!touchEnabled || manualControl || !ready || !canvas || !player) return
      const owner = crypto.randomUUID()
      const source = getProductionMotionRuntime().touch
      const appraisal = createTouchAppraisal(owner, source)
      const cancelReaction = () => { appraisal.cancel(); source.release(owner) }
      const unbind = bindCharacterTouch(canvas,
        (x, y) => player.hitTestTouch(x, y),
        (touch, now) => {
          source.notePresented(owner, player.getPresentedTouch(), now)
          source.update(owner, touch, now)
          if (source.current() || touch.phase === 'end' || touch.phase === 'cancel') {
            appraisal.observe(touch, source.version())
          }
        }, cancelReaction)
      return () => {
        appraisal.dispose(); unbind(); source.release(owner)
      }
    }, [touchEnabled, manualControl, ready, atlasUrl, playback, gpuEpoch])
    const activityRef = useRef(activity)
    const moodRef = useRef(mood)
    const speechActiveRef = useRef(false)
    const speechProsodyRef = useRef<SpeechProsodyPlan | null>(null)
    const singingActiveRef = useRef(false)
    const singingTrackRef = useRef<string | null>(null)
    const musicSignalRef = useRef<MusicMotionSignal | null>(null)
    const motionPolicyRef = useRef<MotionChannelPolicy | null>(null)
    const pendingSpeechTextRef = useRef<
      Array<{ text: string; locale?: string }>
    >([])
    const manualRef = useRef(manualControl)
    const bearingRef = useRef<RigBearing | null>(null)
    const behaviorPlanRef = useRef<BehaviorPlan | null>(null)
    const onPlaybackReadyRef = useRef(onPlaybackReady)
    onPlaybackReadyRef.current = onPlaybackReady
    const onPlaybackErrorRef = useRef(onPlaybackError)
    onPlaybackErrorRef.current = onPlaybackError
    manualRef.current = manualControl || manualRef.current
    const playbackRef = useRef(playback)
    const manifestRef = useRef(manifest)
    playbackRef.current = playback
    manifestRef.current = manifest
    const atlasReadyRef = useRef(false)
    const syncAnimationRef = useRef<() => void>(() => {})
    const presentLiveRef = useRef<(next: boolean) => void>(() => {})
    const recoverGpuRef = useRef<() => boolean>(() => false)

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
      setMusicSignal(drive) {
        musicSignalRef.current = drive
        playerRef.current?.setMusicSignal(drive)
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
          speechArticulationDriverPatch(articulation),
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
        playerRef.current?.setTarget(partial)
      },
      replaceDriver(driver) {
        enterManualControl()
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
      const recoverGpu = () => {
        if (recoveriesRef.current >= GPU_RECOVERIES) return false
        recoveriesRef.current += 1
        setGpuEpoch((epoch) => epoch + 1)
        return true
      }
      recoverGpuRef.current = recoverGpu
      const canvas = canvasRef.current
      const wrapper = wrapperRef.current
      if (!canvas || !wrapper) return undefined
      atlasReadyRef.current = false
      let player: Anime25DPlayer
      try {
        player = new Anime25DPlayer(
          canvas,
          playbackRef.current,
          manifestRef.current,
        )
      } catch (error) {
        readyRef.current = false
        setReady(false)
        if (!recoverGpu()) onPlaybackErrorRef.current?.(error)
        return undefined
      }
      playerRef.current = player
      player.setSpeechActive(speechActiveRef.current)
      player.setSpeechProsody(speechProsodyRef.current)
      player.setSinging(singingActiveRef.current)
      player.setSingingTrack(singingTrackRef.current)
      player.setMusicSignal(musicSignalRef.current)
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
      const presentLive = (next: boolean) => {
        if (cancelled || readyRef.current === next) return
        readyRef.current = next
        setReady(next)
        if (next) onPlaybackReadyRef.current?.()
      }
      presentLiveRef.current = presentLive
      const tick = (now: number) => {
        frame = 0
        if (cancelled || !atlasReadyRef.current || !pageVisible || !inViewport)
          return
        player.tick((now - last) / 1000)
        last = now
        frame = window.requestAnimationFrame(tick)
      }
      const syncAnimation = () => {
        const shouldRun = shouldAnimateAnime25D({
          atlasReady: atlasReadyRef.current,
          pageVisible,
          inViewport,
          cancelled,
        })
        if (!shouldRun) {
          if (frame !== 0) window.cancelAnimationFrame(frame)
          frame = 0
          return
        }
        if (!readyRef.current) {
          player.tick(1 / 60)
          presentLive(true)
        }
        if (frame !== 0) return
        last = performance.now()
        frame = window.requestAnimationFrame(tick)
      }
      syncAnimationRef.current = syncAnimation
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
              inViewport = entries.some((entry) =>
                intersectionKeepsAnime25DVisible(entry),
              )
              syncAnimation()
            })
      viewportObserver?.observe(wrapper)
      const onContextLost = (event: Event) => {
        event.preventDefault()
        if (cancelled) return
        if (!recoverGpu()) onPlaybackErrorRef.current?.(event)
      }
      canvas.addEventListener('webglcontextlost', onContextLost)
      document.addEventListener('visibilitychange', onVisibilityChange)
      resize()
      return () => {
        cancelled = true
        window.cancelAnimationFrame(frame)
        observer.disconnect()
        viewportObserver?.disconnect()
        document.removeEventListener('visibilitychange', onVisibilityChange)
        canvas.removeEventListener('webglcontextlost', onContextLost)
        canvas.removeEventListener('pointermove', onPointerMove)
        canvas.removeEventListener('pointerleave', onPointerLeave)
        player.dispose()
        playerRef.current = null
        readyRef.current = false
        setReady(false)
      }
    }, [gpuEpoch])

    useEffect(() => {
      const player = playerRef.current
      const wrapper = wrapperRef.current
      if (!player) return undefined
      let cancelled = false
      void player
        .replaceLivePackage(playback, manifest, atlasUrl)
        .then(() => {
          if (cancelled) return
          atlasReadyRef.current = true
          recoveriesRef.current = 0
          if (wrapper) {
            const rect = wrapper.getBoundingClientRect()
            player.resize(
              rect.width,
              rect.height,
              window.devicePixelRatio || 1,
            )
          }
          syncAnimationRef.current()
        })
        .catch((error: unknown) => {
          if (cancelled) return
          if (error instanceof Error && error.name === 'AbortError') return
          if (atlasReadyRef.current) return
          presentLiveRef.current(false)
          if (!recoverGpuRef.current()) onPlaybackErrorRef.current?.(error)
        })
      return () => {
        cancelled = true
      }
    }, [atlasUrl, gpuEpoch, manifest, playback])

    return (
      <span
        ref={wrapperRef}
        className={ready ? 'merope-rig is-ready' : 'merope-rig'}
        data-rig-quality="layered-2d"
        data-runtime="Anime2.5DRig"
      >
        <canvas
          key={gpuEpoch}
          ref={canvasRef}
          aria-hidden
          style={{ touchAction: touchEnabled ? 'none' : undefined }}
        />
      </span>
    )
  },
)

export default Anime25DCharacter
