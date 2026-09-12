import type { Anime25DCharacterHandle } from '../anime25drig/Anime25DCharacter'
import type { Anime25DWorkbenchPort } from '../anime25drig/workbenchPort'
import type { RigBearing } from '../motion/bearing'
import type { BehaviorPlan } from '../motion/behavior'
import type { MotionChannelPolicy } from '../motion/policy'
import type { MusicMotionSignal } from '../singing/musicSignal'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { MeropeActivity } from '../types'
import type { SpeechArticulation } from './articulation'
import type { RigMotionPort } from './motionPort'
import type { MeropeRigManifest } from './types'
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import Anime25DCharacter from '../anime25drig/Anime25DCharacter'
import {
  anime25DRuntimeKey,
  shouldUseAnime25DRuntime,
} from '../anime25drig/runtimePolicy'
import { isAnime25DPlayback } from '../anime25drig/types'

const MAX_PENDING_SPEECH_CHUNKS = 32

interface Props {
  activity: MeropeActivity
  fallbackUrl?: string | null
  manifest: MeropeRigManifest | null
  mood: number
  manualControl?: boolean
  touchEnabled?: boolean
  onPlaybackError?: (error: unknown) => void
  onPlaybackReady?: () => void
}

export interface RigCharacterHandle
  extends RigMotionPort, Anime25DWorkbenchPort {}

const RigCharacter = forwardRef<RigCharacterHandle, Props>(
  (
    {
      activity,
      fallbackUrl,
      manifest,
      mood,
      manualControl = false,
      touchEnabled = false,
      onPlaybackError,
      onPlaybackReady,
    },
    ref,
  ) => {
    const animeRef = useRef<Anime25DCharacterHandle>(null)
    const speechActiveRef = useRef(false)
    const speechProsodyRef = useRef<SpeechProsodyPlan | null>(null)
    const singingActiveRef = useRef(false)
    const singingTrackRef = useRef<string | null>(null)
    const musicSignalRef = useRef<MusicMotionSignal | null>(null)
    const motionPolicyRef = useRef<MotionChannelPolicy | null>(null)
    const moodRef = useRef(mood)
    const activityRef = useRef(activity)
    const pendingSpeechTextRef = useRef<
      Array<{ text: string; locale?: string }>
    >([])
    const bearingRef = useRef<RigBearing | null>(null)
    const latestBehaviorPlanRef = useRef<BehaviorPlan | null>(null)
    const latestSpeechRef = useRef<
      | { kind: 'auto'; active: boolean }
      | { kind: 'energy'; energy: number | null }
      | { kind: 'articulation'; articulation: SpeechArticulation }
    >({ kind: 'auto', active: false })
    const playback =
      manifest?.anime25dPlayback &&
      isAnime25DPlayback(manifest.anime25dPlayback)
        ? manifest.anime25dPlayback
        : null
    const atlasUrl = manifest?.textures[0]?.url || ''
    const runtimeKey = anime25DRuntimeKey(
      manifest?.sourceMasterAssetId,
      manifest?.characterAssetContractVersion,
      atlasUrl,
    )
    const [failedRuntimeKey, setFailedRuntimeKey] = useState<string | null>(
      null,
    )
    const useAnimeRuntime = shouldUseAnime25DRuntime({
      hasManifest: Boolean(manifest),
      hasPlayback: Boolean(playback),
      atlasUrl,
      runtimeKey,
      failedRuntimeKey,
    })
    const handlePlaybackError = useCallback(
      (error: unknown) => {
        setFailedRuntimeKey(runtimeKey)
        onPlaybackError?.(error)
      },
      [onPlaybackError, runtimeKey],
    )

    useEffect(() => {
      if (useAnimeRuntime) return
      pendingSpeechTextRef.current = []
      latestBehaviorPlanRef.current = null
    }, [useAnimeRuntime])

    useEffect(() => {
      animeRef.current?.setSpeechActive(speechActiveRef.current)
      animeRef.current?.setSpeechProsody(speechProsodyRef.current)
      animeRef.current?.setSinging(singingActiveRef.current)
      animeRef.current?.setSingingTrack(singingTrackRef.current)
      animeRef.current?.setMusicSignal(musicSignalRef.current)
      if (motionPolicyRef.current) {
        animeRef.current?.setMotionPolicy(motionPolicyRef.current)
      }
      animeRef.current?.setBearing(bearingRef.current)
      animeRef.current?.setMood(moodRef.current, activityRef.current)
      const latest = latestSpeechRef.current
      if (latest.kind === 'auto') {
        animeRef.current?.setAutoSpeech(latest.active)
      } else if (latest.kind === 'energy') {
        animeRef.current?.setSpeechEnergy(latest.energy)
      } else {
        animeRef.current?.setSpeechArticulation(latest.articulation)
      }
      for (const chunk of pendingSpeechTextRef.current) {
        animeRef.current?.enqueueSpeechText(chunk.text, chunk.locale)
      }
      pendingSpeechTextRef.current = []
      if (latestBehaviorPlanRef.current) {
        animeRef.current?.playBehaviorPlan(latestBehaviorPlanRef.current)
      }
    }, [atlasUrl, playback])

    useImperativeHandle(ref, () => ({
      setBearing: (bearing) => {
        bearingRef.current = bearing
        animeRef.current?.setBearing(bearing)
      },
      setSpeechActive: (active) => {
        speechActiveRef.current = active
        animeRef.current?.setSpeechActive(active)
      },
      setSinging: (active) => {
        singingActiveRef.current = active
        animeRef.current?.setSinging(active)
      },
      setSingingTrack: (trackId) => {
        singingTrackRef.current = trackId
        animeRef.current?.setSingingTrack(trackId)
      },
      setMusicSignal: (drive) => {
        musicSignalRef.current = drive
        animeRef.current?.setMusicSignal(drive)
      },
      setAutoSpeech: (active) => {
        latestSpeechRef.current = { kind: 'auto', active }
        animeRef.current?.setAutoSpeech(active)
      },
      setSpeechEnergy: (energy) => {
        latestSpeechRef.current = { kind: 'energy', energy }
        animeRef.current?.setSpeechEnergy(energy)
      },
      setSpeechArticulation: (articulation) => {
        latestSpeechRef.current = { kind: 'articulation', articulation }
        animeRef.current?.setSpeechArticulation(articulation)
      },
      setSpeechProsody: (prosody) => {
        speechProsodyRef.current = prosody
        animeRef.current?.setSpeechProsody(prosody)
      },
      enqueueSpeechText: (text, locale) => {
        if (animeRef.current) {
          animeRef.current.enqueueSpeechText(text, locale)
        } else if (useAnimeRuntime) {
          pendingSpeechTextRef.current = [
            ...pendingSpeechTextRef.current,
            { text, locale },
          ].slice(-MAX_PENDING_SPEECH_CHUNKS)
        }
      },
      playBehaviorPlan: (plan) => {
        if (!useAnimeRuntime) {
          return plan.behaviors.map((behavior) => ({
            behaviorId: behavior.id,
            result: 'rejected' as const,
            atMs: currentNow(),
            reason: 'unsupported-form' as const,
          }))
        }
        latestBehaviorPlanRef.current = plan
        return animeRef.current?.playBehaviorPlan(plan) ?? []
      },
      stopBehaviorPlan: (planId) => {
        if (planId && latestBehaviorPlanRef.current?.id !== planId) return
        latestBehaviorPlanRef.current = null
        animeRef.current?.stopBehaviorPlan(planId)
      },
      setDriver: (partial) => animeRef.current?.setDriver(partial),
      replaceDriver: (driver) => animeRef.current?.replaceDriver(driver),
      blinkNow: () => animeRef.current?.blinkNow(),
      debugSnapshot: () => animeRef.current?.debugSnapshot() ?? null,
      setMotionPolicy: (policy) => {
        motionPolicyRef.current = policy
        animeRef.current?.setMotionPolicy(policy)
      },
      setMood: (nextMood, nextActivity) => {
        moodRef.current = nextMood
        activityRef.current = nextActivity
        animeRef.current?.setMood(nextMood, nextActivity)
      },
    }))

    if (useAnimeRuntime && manifest && playback) {
      return (
        <Anime25DCharacter
          ref={animeRef}
          activity={activity}
          manifest={manifest}
          playback={playback}
          atlasUrl={atlasUrl}
          mood={mood}
          manualControl={manualControl}
          touchEnabled={touchEnabled}
          onPlaybackError={handlePlaybackError}
          onPlaybackReady={onPlaybackReady}
        />
      )
    }

    if (!fallbackUrl) return null

    return (
      <StaticFaceImage src={fallbackUrl} onPlaybackReady={onPlaybackReady} />
    )
  },
)

function StaticFaceImage({
  src,
  onPlaybackReady,
}: {
  src: string
  onPlaybackReady?: () => void
}) {
  useLayoutEffect(() => {
    onPlaybackReady?.()
  }, [onPlaybackReady, src])
  return (
    <span className="merope-rig is-ready" data-rig-quality="static">
      <img src={src} alt="" draggable={false} />
    </span>
  )
}

function currentNow(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}

export default RigCharacter
