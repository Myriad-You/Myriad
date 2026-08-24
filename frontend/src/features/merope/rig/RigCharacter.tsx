import type { PerformanceDirective } from '../../../services/agent/types'
import type { Anime25DCharacterHandle } from '../anime25drig/Anime25DCharacter'
import type {
  Anime25DDebugSnapshot,
  Anime25DDriver,
} from '../anime25drig/player'
import type { MeropeActivity } from '../types'
import type { SpeechArticulation } from './articulation'
import type { GazeSource, GazeTarget } from './motion'
import type { MeropeRigManifest } from './types'
import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import Anime25DCharacter from '../anime25drig/Anime25DCharacter'
import { isAnime25DPlayback } from '../anime25drig/types'

interface Props {
  activity: MeropeActivity
  fallbackUrl: string
  manifest: MeropeRigManifest | null
  mood: number
  manualControl?: boolean
}

export interface RigCharacterHandle {
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

const RigCharacter = forwardRef<RigCharacterHandle, Props>(
  ({ activity, fallbackUrl, manifest, mood, manualControl = false }, ref) => {
    const animeRef = useRef<Anime25DCharacterHandle>(null)
    const speechActiveRef = useRef(false)
    const latestSpeechRef = useRef<
      | { kind: 'auto'; active: boolean }
      | { kind: 'energy'; energy: number | null }
      | { kind: 'articulation'; articulation: SpeechArticulation }
    >({ kind: 'auto', active: false })
    const playback =
      manifest?.anime25dPlayback && isAnime25DPlayback(manifest.anime25dPlayback)
        ? manifest.anime25dPlayback
        : null
    const atlasUrl = manifest?.textures[0]?.url || ''

    useEffect(() => {
      animeRef.current?.setSpeechActive(speechActiveRef.current)
      const latest = latestSpeechRef.current
      if (latest.kind === 'auto') {
        animeRef.current?.setAutoSpeech(latest.active)
      } else if (latest.kind === 'energy') {
        animeRef.current?.setSpeechEnergy(latest.energy)
      } else {
        animeRef.current?.setSpeechArticulation(latest.articulation)
      }
    }, [atlasUrl, playback])

    useImperativeHandle(ref, () => ({
      setSpeechActive: (active) => {
        speechActiveRef.current = active
        animeRef.current?.setSpeechActive(active)
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
      setGazeTarget: (target, source) =>
        animeRef.current?.setGazeTarget(target, source),
      playMotionPlan: (performance) => animeRef.current?.playMotionPlan(performance),
      stopMotionPlan: () => animeRef.current?.stopMotionPlan(),
      captureFrame: () => animeRef.current?.captureFrame() ?? null,
      setDriver: (partial) => animeRef.current?.setDriver(partial),
      replaceDriver: (driver) => animeRef.current?.replaceDriver(driver),
      resetDriver: () => animeRef.current?.resetDriver(),
      getDriver: () => animeRef.current?.getDriver() ?? null,
      blinkNow: () => animeRef.current?.blinkNow(),
      debugSnapshot: () => animeRef.current?.debugSnapshot() ?? null,
      setMouse: (x, y, inside) => animeRef.current?.setMouse(x, y, inside),
    }))

    if (manifest && playback && atlasUrl) {
      return (
        <Anime25DCharacter
          ref={animeRef}
          activity={activity}
          fallbackUrl={fallbackUrl}
          manifest={manifest}
          playback={playback}
          atlasUrl={atlasUrl}
          mood={mood}
          manualControl={manualControl}
        />
      )
    }

    return (
      <span className="merope-rig is-ready" data-rig-quality="static">
        <img src={fallbackUrl} alt="" draggable={false} />
      </span>
    )
  },
)

export default RigCharacter
