import type { TranslationKeys } from '../../../i18n'
import type { Anime25DDriver } from '../anime25drig/driver'
import type { Anime25DMotionEnvelopeProbeId } from '../anime25drig/motionEnvelope'
import type { Anime25DDebugSnapshot } from '../anime25drig/player'
import { formatMessage, getDefaultLocale } from '../../../i18n'
import {
  ANGRY_EXPRESSION_PRESET,
  CRY_EXPRESSION_PRESET,
  DIZZY_EXPRESSION_PRESET,
  LOVESTRUCK_EXPRESSION_PRESET,
  MANIAC_EXPRESSION_PRESET,
  SILLY_EXPRESSION_PRESET,
  SPEECHLESS_EXPRESSION_PRESET,
  SQUEEZE_EXPRESSION_PRESET,
  THINKING_EXPRESSION_PRESET,
} from '../anime25drig/expressionPresets'

type Labels = TranslationKeys['merope']
type StringLabel = {
  [K in keyof Labels]: Labels[K] extends string ? K : never
}[keyof Labels]
export type DriverSliderKey = {
  [K in keyof Anime25DDriver]: Anime25DDriver[K] extends number ? K : never
}[keyof Anime25DDriver]

export const PRESETS: Array<{ id: string; driver: Partial<Anime25DDriver> }> = [
  {
    id: 'neutral',
    driver: {
      eyeOpenL: 1,
      eyeOpenR: 1,
      brow: 0,
      mouthOpen: 0,
      mouthForm: 0,
      irisScale: 1,
    },
  },
  {
    id: 'smile',
    driver: {
      eyeOpenL: 0,
      eyeOpenR: 0,
      brow: 0.45,
      mouthOpen: 0,
      mouthForm: 0.9,
      irisScale: 1,
    },
  },
  {
    id: 'usume',
    driver: {
      eyeOpenL: 0.5,
      eyeOpenR: 0.5,
      brow: 0.35,
      mouthOpen: 1,
      mouthForm: 0.8,
      irisScale: 1,
    },
  },
  {
    id: 'surprise',
    driver: {
      eyeOpenL: 1,
      eyeOpenR: 1,
      brow: 1,
      mouthOpen: 0.75,
      mouthForm: -0.1,
      irisScale: 0.7,
    },
  },
  {
    id: 'jito',
    driver: {
      eyeOpenL: 0.4,
      eyeOpenR: 0.4,
      brow: -0.6,
      mouthOpen: 0,
      mouthForm: -0.4,
      irisScale: 1,
    },
  },
  { id: 'thinking', driver: { ...THINKING_EXPRESSION_PRESET } },
  { id: 'dizzy', driver: { ...DIZZY_EXPRESSION_PRESET } },
  { id: 'squeeze', driver: { ...SQUEEZE_EXPRESSION_PRESET } },
  { id: 'cry', driver: { ...CRY_EXPRESSION_PRESET } },
  { id: 'angry', driver: { ...ANGRY_EXPRESSION_PRESET } },
  { id: 'speechless', driver: { ...SPEECHLESS_EXPRESSION_PRESET } },
  { id: 'maniac', driver: { ...MANIAC_EXPRESSION_PRESET } },
  { id: 'silly', driver: { ...SILLY_EXPRESSION_PRESET } },
  { id: 'lovestruck', driver: { ...LOVESTRUCK_EXPRESSION_PRESET } },
  {
    id: 'winkL',
    driver: {
      eyeOpenL: 0,
      eyeOpenR: 1,
      brow: 0.2,
      mouthOpen: 0.4,
      mouthForm: 0.7,
      irisScale: 1,
    },
  },
  {
    id: 'winkR',
    driver: {
      eyeOpenL: 1,
      eyeOpenR: 0,
      brow: 0.2,
      mouthOpen: 0.4,
      mouthForm: 0.7,
      irisScale: 1,
    },
  },
]

export function presetLabel(labels: Labels, id: string): string {
  if (id === 'neutral') return labels.anime25dPresetIdle
  if (id === 'smile') return labels.anime25dPresetSmile
  if (id === 'usume') return labels.anime25dPresetUsume
  if (id === 'surprise') return labels.anime25dPresetShock
  if (id === 'jito') return labels.anime25dPresetDeadpan
  if (id === 'thinking') return labels.anime25dPresetThinking
  if (id === 'dizzy') return labels.anime25dPresetDizzy
  if (id === 'squeeze') return labels.anime25dPresetSqueeze
  if (id === 'cry') return labels.anime25dPresetCry
  if (id === 'angry') return labels.anime25dPresetAngry
  if (id === 'speechless') return labels.anime25dPresetSpeechless
  if (id === 'maniac') return labels.anime25dPresetManiac
  if (id === 'silly') return labels.anime25dPresetSilly
  if (id === 'lovestruck') return labels.anime25dPresetLovestruck
  if (id === 'winkL') return labels.anime25dPresetWinkLeft
  if (id === 'winkR') return labels.anime25dPresetWinkRight
  return id
}

export function envelopeProbeLabel(
  labels: Labels,
  id: Anime25DMotionEnvelopeProbeId,
): string {
  if (id === 'turn-left') return labels.anime25dProbeTurnLeft
  if (id === 'turn-right') return labels.anime25dProbeTurnRight
  if (id === 'pitch-up') return labels.anime25dProbePitchUp
  if (id === 'pitch-down') return labels.anime25dProbePitchDown
  if (id === 'full-left') return labels.anime25dProbeFullLeft
  return labels.anime25dProbeFullRight
}

interface DriverSlider {
  key: DriverSliderKey
  label: StringLabel
  min: number
  max: number
}

export const DRIVER_SLIDERS: readonly DriverSlider[] = [
  { key: 'angleX', label: 'anime25dHeadX', min: -1, max: 1 },
  { key: 'angleY', label: 'anime25dHeadY', min: -1, max: 1 },
  { key: 'angleZ', label: 'anime25dHeadZ', min: -1, max: 1 },
  { key: 'eyeOpenL', label: 'anime25dEyeL', min: 0, max: 1 },
  { key: 'eyeOpenR', label: 'anime25dEyeR', min: 0, max: 1 },
  { key: 'eyeX', label: 'anime25dEyeX', min: -1, max: 1 },
  { key: 'eyeY', label: 'anime25dEyeY', min: -1, max: 1 },
  { key: 'irisScale', label: 'anime25dPupil', min: 0.5, max: 1.3 },
  { key: 'eyeScaleL', label: 'anime25dEyeScaleL', min: 0.5, max: 1.5 },
  { key: 'eyeScaleR', label: 'anime25dEyeScaleR', min: 0.5, max: 1.5 },
  { key: 'eyeEase', label: 'anime25dEyeEase', min: 0, max: 1 },
  { key: 'eyeCY', label: 'anime25dEyeCY', min: -1, max: 1 },
  { key: 'eyeCAng', label: 'anime25dEyeCAng', min: -1, max: 1 },
  { key: 'brow', label: 'anime25dBrow', min: -1, max: 1 },
  { key: 'browAngSym', label: 'anime25dBrowAngSym', min: -1, max: 1 },
  { key: 'browAngL', label: 'anime25dBrowL', min: -1, max: 1 },
  { key: 'browAngR', label: 'anime25dBrowR', min: -1, max: 1 },
  { key: 'mouthOpen', label: 'anime25dMouth', min: 0, max: 1 },
  { key: 'mouthForm', label: 'anime25dMouthForm', min: -1, max: 1 },
  { key: 'mouthCY', label: 'anime25dMouthCY', min: -1, max: 1 },
  { key: 'mouthEase', label: 'anime25dMouthEase', min: 0, max: 1 },
  { key: 'mouthCAng', label: 'anime25dMouthCAng', min: -1, max: 1 },
  { key: 'mouthScale', label: 'anime25dMouthScale', min: 0.5, max: 1.5 },
  { key: 'fhAmp', label: 'anime25dFhAmp', min: 0, max: 3 },
  { key: 'fhSoft', label: 'anime25dFhSoft', min: 0, max: 2 },
  { key: 'bangL', label: 'anime25dBangL', min: -1, max: 1 },
  { key: 'bangC', label: 'anime25dBangC', min: -1, max: 1 },
  { key: 'bangR', label: 'anime25dBangR', min: -1, max: 1 },
  { key: 'body', label: 'anime25dLean', min: -1, max: 1 },
  { key: 'bodyYaw', label: 'anime25dBodyYaw', min: 0, max: 1 },
  { key: 'armY', label: 'anime25dArmY', min: -1, max: 1 },
  { key: 'armPos', label: 'anime25dArmPos', min: -1, max: 1 },
  { key: 'bust', label: 'anime25dBust', min: 0, max: 4 },
  { key: 'bustY', label: 'anime25dBustY', min: -3, max: 3 },
  { key: 'physAmp', label: 'anime25dPhysAmp', min: 0, max: 3 },
  { key: 'soft', label: 'anime25dSoft', min: 0, max: 3 },
]

function fillInspect(
  template: string,
  vars: Record<string, string | number>,
): string {
  return formatMessage(getDefaultLocale(), template, vars)
}

/** The rig inspector's rows for one debug snapshot. */
export function inspectFields(labels: Labels, snapshot: Anime25DDebugSnapshot) {
  return [
    {
      key: 'layers',
      label: labels.anime25dInspectLayers,
      value: fillInspect(labels.anime25dInspectLayersValue, {
        count: snapshot.layerCount,
      }),
      copyable: false,
    },
    {
      key: 'strands',
      label: labels.anime25dInspectStrands,
      value: fillInspect(labels.anime25dInspectStrandsValue, {
        strands: snapshot.strandCount,
        layers: snapshot.hairLayerCount,
      }),
      copyable: false,
    },
    {
      key: 'eyes',
      label: labels.anime25dInspectEyes,
      value: fillInspect(labels.anime25dInspectEyesValue, {
        open: snapshot.eyeOpenLayers,
        close: snapshot.eyeCloseLayers,
      }),
      copyable: false,
    },
    {
      key: 'mouth',
      label: labels.anime25dInspectMouth,
      value: fillInspect(labels.anime25dInspectMouthValue, {
        open:
          snapshot.mouthOpenLayers +
          snapshot.mouthWideLayers +
          snapshot.mouthRoundLayers +
          snapshot.mouthNarrowLayers,
        close: snapshot.mouthCloseLayers,
      }),
      copyable: false,
    },
    {
      key: 'canvas',
      label: labels.anime25dInspectCanvas,
      value: fillInspect(labels.anime25dInspectCanvasValue, {
        width: Math.round(snapshot.canvas.width),
        height: Math.round(snapshot.canvas.height),
      }),
      copyable: false,
    },
    {
      key: 'motion-envelope',
      label: labels.anime25dInspectEnvelope,
      value: fillInspect(labels.anime25dInspectEnvelopeValue, {
        pitch: Math.round(snapshot.motionEnvelope.pitchLimit * 100),
        torso: Math.round(snapshot.motionEnvelope.torsoLimit * 100),
        arm: Math.round(snapshot.motionEnvelope.armLimit * 100),
        transfer: Math.round(snapshot.motionEnvelope.transferredEnergy * 100),
      }),
      copyable: false,
    },
    {
      key: 'performance',
      label: labels.anime25dInspectPerformance,
      value: fillInspect(labels.anime25dInspectPerformanceValue, {
        frame: snapshot.performance.frameCpuMs.toFixed(2),
        deform: snapshot.performance.deformMs.toFixed(2),
        upload: snapshot.performance.uploadSubmitMs.toFixed(2),
      }),
      copyable: false,
    },
    {
      key: 'workload',
      label: labels.anime25dInspectWorkload,
      value: fillInspect(labels.anime25dInspectWorkloadValue, {
        vertices: Math.round(snapshot.performance.deformedVertices),
        skipped: Math.round(snapshot.performance.skippedVertices),
        kilobytes: Math.round(snapshot.performance.uploadedBytes / 1024),
        saved: Math.round(snapshot.performance.savedUploadBytes / 1024),
        draws: Math.round(snapshot.performance.drawCalls),
      }),
      copyable: false,
    },
  ]
}
