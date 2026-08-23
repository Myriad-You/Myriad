import type { CompanionActivity } from '../types'
import type { SpeechArticulation } from './articulation'
import type { GazeSource, GazeTarget } from './motion'
import type { CompanionRigManifest } from './types'
import { forwardRef, useImperativeHandle, useRef } from 'react'
import Anime25DCharacter from '../anime25drig/Anime25DCharacter'
import type { Anime25DCharacterHandle } from '../anime25drig/Anime25DCharacter'
import type {
  Anime25DDebugSnapshot,
  Anime25DDriver,
} from '../anime25drig/player'
import { isAnime25DPlayback } from '../anime25drig/types'

interface Props {
  activity: CompanionActivity
  fallbackUrl: string
  manifest: CompanionRigManifest | null
  mood: number
  manualControl?: boolean
}

export interface RigCharacterHandle {
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

const RigCharacter = forwardRef<RigCharacterHandle, Props>(
  ({ activity, fallbackUrl, manifest, mood, manualControl = false }, ref) => {
    const animeRef = useRef<Anime25DCharacterHandle>(null)
    const playback =
      manifest?.anime25dPlayback && isAnime25DPlayback(manifest.anime25dPlayback)
        ? manifest.anime25dPlayback
        : null
    const atlasUrl = manifest?.textures[0]?.url || ''

    useImperativeHandle(ref, () => ({
      setSpeechEnergy: (value) => animeRef.current?.setSpeechEnergy(value),
      setSpeechArticulation: (value) =>
        animeRef.current?.setSpeechArticulation(value),
      setGazeTarget: (target, source) =>
        animeRef.current?.setGazeTarget(target, source),
      playMotionPlan: () => animeRef.current?.playMotionPlan(),
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

    if (playback && atlasUrl) {
      return (
        <Anime25DCharacter
          ref={animeRef}
          activity={activity}
          fallbackUrl={fallbackUrl}
          playback={playback}
          atlasUrl={atlasUrl}
          mood={mood}
          manualControl={manualControl}
        />
      )
    }

    return (
      <span className="dlc-rig is-ready" data-rig-quality="static">
        <img src={fallbackUrl} alt="" draggable={false} />
      </span>
    )
  },
)

export default RigCharacter
