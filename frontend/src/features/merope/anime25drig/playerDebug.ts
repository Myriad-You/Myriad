import type { Anime25DDriver } from './driver'
import type { Anime25DMotionEnvelopeProfile } from './motionEnvelope'
import type { Anime25DPerformanceSnapshot } from './performanceTelemetry'
import type { Anime25DFade, Anime25DPlayback } from './types'

export interface Anime25DDebugSnapshot {
  layerCount: number
  hairLayerCount: number
  strandCount: number
  eyeOpenLayers: number
  eyeCloseLayers: number
  eyeDizzyLayers: number
  eyeSqueezeLayers: number
  eyeCryLayers: number
  eyeSillyLayers: number
  lovestruckHeartLayers: number
  lovestruckFaceLayers: number
  lovestruckDroolLayers: number
  maniacEyeShadowLayers: number
  angerMarkLayers: number
  speechlessSweatLayers: number
  mouthOpenLayers: number
  mouthWideLayers: number
  mouthRoundLayers: number
  mouthNarrowLayers: number
  mouthCloseLayers: number
  mouthCryLayers: number
  mouthManiacLayers: number
  mouthSillyLayers: number
  canvas: { width: number; height: number }
  motionEnvelope: {
    highCollar: boolean
    armMotion: boolean
    pitchLimit: number
    torsoLimit: number
    armLimit: number
    transferredEnergy: number
  }
  performance: Anime25DPerformanceSnapshot
  current: Anime25DDriver
}

/** Snapshot keys that count the layers fading in for one expression. */
const DEBUG_FADE_COUNTS = {
  eyeOpenLayers: 'eyeOpen',
  eyeCloseLayers: 'eyeClose',
  eyeDizzyLayers: 'eyeDizzy',
  eyeSqueezeLayers: 'eyeSqueeze',
  eyeCryLayers: 'eyeCry',
  eyeSillyLayers: 'eyeSilly',
  lovestruckHeartLayers: 'lovestruckHeart',
  lovestruckFaceLayers: 'lovestruckFace',
  lovestruckDroolLayers: 'lovestruckDrool',
  maniacEyeShadowLayers: 'maniacEyeShadow',
  angerMarkLayers: 'angerMark',
  speechlessSweatLayers: 'speechlessSweat',
  mouthOpenLayers: 'mouthOpen',
  mouthWideLayers: 'mouthWide',
  mouthRoundLayers: 'mouthRound',
  mouthNarrowLayers: 'mouthNarrow',
  mouthCloseLayers: 'mouthClose',
  mouthCryLayers: 'mouthCry',
  mouthManiacLayers: 'mouthManiac',
  mouthSillyLayers: 'mouthSilly',
} as const satisfies Record<string, Anime25DFade>

/** What the workbench's inspector shows about the playing package. */
export function describeAnime25DPlayback(
  playback: Readonly<Anime25DPlayback>,
  motionEnvelopeProfile: Readonly<Anime25DMotionEnvelopeProfile>,
  transferredEnergy: number,
  performance: Anime25DPerformanceSnapshot,
  current: Anime25DDriver,
): Anime25DDebugSnapshot {
  const layers = playback.layers
  const fadeCounts = Object.fromEntries(
    Object.entries(DEBUG_FADE_COUNTS).map(([key, fade]) => [
      key,
      layers.filter((layer) => layer.fade === fade).length,
    ]),
  ) as Record<keyof typeof DEBUG_FADE_COUNTS, number>
  return {
    layerCount: layers.length,
    hairLayerCount: layers.filter((layer) => layer.phys === 'hair').length,
    strandCount: layers.reduce((sum, layer) => sum + layer.strands.length, 0),
    ...fadeCounts,
    canvas: { ...playback.pixelCanvas },
    motionEnvelope: {
      highCollar: motionEnvelopeProfile.highCollar,
      armMotion: motionEnvelopeProfile.armMotion,
      pitchLimit: motionEnvelopeProfile.pitch.limit,
      torsoLimit: motionEnvelopeProfile.torso.limit,
      armLimit: motionEnvelopeProfile.rigidArm.limit,
      transferredEnergy,
    },
    performance,
    current,
  }
}
