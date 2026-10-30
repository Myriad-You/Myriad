import type { Anime25DDriver } from './driver'
import type {
  MouthTransitionSample,
  SpeechMouthMaterial,
} from './mouthTransition'
import type { Anime25DPlayback, Anime25DPlaybackLayer } from './types'
import { regularMouthMaterial } from './mouthTransition'

export interface MouthMorphState {
  centerX: number
  centerY: number
  width: number
  height: number
  openMix: number
  wide: number
  round: number
  narrow: number
  /**
   * The speaking materials' own opening. A closed mouth is a lip line with no
   * gap, so here it counts as a slit rather than its drawn curve's box: an
   * opening mouth grows from that slit instead of appearing at full height.
   */
  openCenterY: number
  openHeight: number
}

/** A just-parted mouth, as a share of the closed lip line's drawn height. */
const PARTED_LIP_SLIT = 0.2

export interface Anime25DMouthMorphSources {
  closed?: Anime25DPlaybackLayer
  ordinary?: Anime25DPlaybackLayer
  wide?: Anime25DPlaybackLayer
  round?: Anime25DPlaybackLayer
  narrow?: Anime25DPlaybackLayer
  maniac?: Anime25DPlaybackLayer
}

export interface Anime25DOpacityFrame {
  dizzy: number
  cry: number
  mouthCry: number
  squeeze: number
  anger: number
  speechless: number
  accentNotes: number
  accentQuestion: number
  accentSleep: number
  accentSparkle: number
  accentBlush: number
  maniac: number
  silly: number
  lovestruck: number
  sillyMouth: number
  symbolBlocker: number
  eyeOpenL: number
  eyeOpenR: number
  lovestruckHeartL: number
  lovestruckHeartR: number
  activeMouthMaterial: SpeechMouthMaterial
  mouthUnderlay: SpeechMouthMaterial
  /** The open drawing the underlay is taking over from, and how far that handoff is. */
  mouthPrevious: SpeechMouthMaterial
  mouthHandoff: number
  /** The open mouth is drawn live as every speaking shape; the other speaking drawings stay hidden. */
  continuousMouth: boolean
  /** The closed line still showing over lips that have only just parted. */
  closedLinger: number
  /** How far the parting slit has come in over that line. */
  partingReveal: number
}

export function compileAnime25DMouthMorphSources(
  layers: readonly Anime25DPlaybackLayer[],
): Anime25DMouthMorphSources {
  const sources: Anime25DMouthMorphSources = {}
  for (const layer of layers) {
    if (layer.fade === 'mouthClose') sources.closed ??= layer
    else if (layer.fade === 'mouthOpen') sources.ordinary ??= layer
    else if (layer.fade === 'mouthWide') sources.wide ??= layer
    else if (layer.fade === 'mouthRound') sources.round ??= layer
    else if (layer.fade === 'mouthNarrow') sources.narrow ??= layer
    else if (layer.fade === 'mouthManiac') sources.maniac ??= layer
  }
  return sources
}

export function resolveMouthMorph(
  sources: Readonly<Anime25DMouthMorphSources>,
  driver: Anime25DDriver,
  fallback: Anime25DPlayback['anchors']['mouth'],
  face: Anime25DPlayback['anchors']['face'],
  output: MouthMorphState,
): void {
  const openMix = mouthOpenMix(driver)
  const shapeScale = mouthShapeScale(driver)
  const articulation = 1 - smoothstep(driver.mouthSeal)
  const wide = driver.mouthWide * shapeScale * articulation
  const round = driver.mouthRound * shapeScale * articulation
  const narrow = driver.mouthNarrow * shapeScale * articulation
  const open = Math.max(0, 1 - wide - round - narrow)
  const maniac = smoothstep(driver.maniac)
  const regular = 1 - maniac
  output.openMix = Math.max(openMix, maniac)
  output.wide = wide
  output.round = round
  output.narrow = narrow

  let total = 0
  output.centerX = 0
  output.centerY = 0
  output.width = 0
  output.height = 0
  const closed = sources.closed
  const ordinary = sources.ordinary
  const wideLayer = sources.wide
  const roundLayer = sources.round
  const narrowLayer = sources.narrow
  const maniacLayer = sources.maniac
  if (closed)
    total += addMouthMorphSource(output, closed, (1 - openMix) * regular)
  if (ordinary)
    total += addMouthMorphSource(output, ordinary, openMix * open * regular)
  if (wideLayer)
    total += addMouthMorphSource(output, wideLayer, openMix * wide * regular)
  if (roundLayer)
    total += addMouthMorphSource(output, roundLayer, openMix * round * regular)
  if (narrowLayer) {
    total += addMouthMorphSource(
      output,
      narrowLayer,
      openMix * narrow * regular,
    )
  }
  if (maniacLayer) total += addMouthMorphSource(output, maniacLayer, maniac)
  if (total <= 0) {
    output.centerX = fallback.cx
    output.centerY = fallback.cy
    output.width = Math.max(1, fallback.x1 - fallback.x0)
    output.height = Math.max(1, fallback.y1 - fallback.y0)
    output.openCenterY = output.centerY
    output.openHeight = output.height
    return
  }
  output.centerX /= total
  output.centerY /= total
  output.width = Math.max(1, output.width / total)
  output.height = Math.max(1, output.height / total)
  if (closed && output.openMix > 0) {
    const blendedTop = output.centerY - output.height / 2
    const blendedBottom = output.centerY + output.height / 2
    const neutralTop = closed.y
    const neutralBottom = closed.y + closed.h
    const upperRelease = 0.32 + maniac * 0.68
    const lowerRelease = 0.88 + maniac * 0.12
    const anchoredTop = neutralTop + (blendedTop - neutralTop) * upperRelease
    const releasedBottom =
      neutralBottom + (blendedBottom - neutralBottom) * lowerRelease
    output.centerY = (anchoredTop + releasedBottom) / 2
    output.height = Math.max(1, releasedBottom - anchoredTop)
  }
  if (closed) {
    // Parted lips first, the full drawn opening once the mouth is open.
    const slit = Math.max(1, closed.h * PARTED_LIP_SLIT)
    const partedTop = closed.y + (closed.h - slit) / 2
    const aperture = output.openMix
    const openTop =
      partedTop + (output.centerY - output.height / 2 - partedTop) * aperture
    output.openHeight = slit + (output.height - slit) * aperture
    output.openCenterY = openTop + output.openHeight / 2
  } else {
    output.openCenterY = output.centerY
    output.openHeight = output.height
  }
  if (maniac > 0) {
    // An extreme mouth must still fit the character's lower face.
    const faceWidth = Math.max(1, face.x1 - face.x0)
    const faceHeight = Math.max(1, face.y1 - face.y0)
    const maximumWidth = faceWidth * 0.54
    const chinMargin = Math.max(2, faceHeight * 0.01)
    const lowerFaceRoom = Math.max(1, face.y1 - fallback.cy - chinMargin)
    const maximumHeight = Math.max(1, lowerFaceRoom / 0.44)
    const guardedWidth = Math.min(output.width, maximumWidth)
    const guardedHeight = Math.min(output.height, maximumHeight)
    const guardedCenterY = Math.min(output.centerY, fallback.cy)
    output.width += (guardedWidth - output.width) * maniac
    output.height += (guardedHeight - output.height) * maniac
    output.centerY += (guardedCenterY - output.centerY) * maniac
  }
}

function addMouthMorphSource(
  output: MouthMorphState,
  source: Anime25DPlaybackLayer,
  weight: number,
): number {
  if (weight <= 0) return 0
  output.centerX += (source.x + source.w / 2) * weight
  output.centerY += (source.y + source.h / 2) * weight
  output.width += source.w * weight
  output.height += source.h * weight
  return weight
}

function mouthOpenMix(driver: Anime25DDriver): number {
  const opening = smoothstep(
    (driver.mouthOpen - (0.02 + driver.mouthEase * 0.08)) /
      (0.53 + driver.mouthEase * 0.17),
  )
  return opening * (1 - smoothstep(driver.mouthSeal))
}

function mouthShapeScale(driver: Anime25DDriver): number {
  const total = driver.mouthWide + driver.mouthRound + driver.mouthNarrow
  return total > 1 ? 1 / total : 1
}

export function applyMouthTransitionBridge(
  output: MouthMorphState,
  transition: Readonly<MouthTransitionSample>,
): void {
  output.width = Math.max(1, output.width * transition.widthScale)
  output.height = Math.max(1, output.height * transition.heightScale)
  output.openHeight = Math.max(1, output.openHeight * transition.heightScale)
  output.centerX += transition.centerOffsetX
  output.centerY += transition.centerOffsetY
  output.openCenterY += transition.centerOffsetY
  const retainedShape = 1 - transition.shapeNeutralization
  output.wide *= retainedShape
  output.round *= retainedShape
  output.narrow *= retainedShape
}

export function fadeOpacity(
  layer: Anime25DPlaybackLayer,
  driver: Anime25DDriver,
  activeMouthMaterial?: SpeechMouthMaterial,
  sillyMouthShare = 1,
): number {
  if (!layer.fade) return 1
  const dizzy = smoothstep(driver.eyeDizzy)
  const cry = smoothstep(driver.eyeCry)
  const mouthCry = cry * (1 - dizzy)
  const squeeze = smoothstep(driver.eyeSqueeze)
  const anger = smoothstep(driver.anger)
  const speechless = smoothstep(driver.speechless)
  const maniac = smoothstep(driver.maniac)
  const silly =
    smoothstep(driver.silly) *
    (1 - dizzy) *
    (1 - cry) *
    (1 - squeeze) *
    (1 - maniac)
  const lovestruck =
    smoothstep(driver.lovestruck) *
    (1 - dizzy) *
    (1 - cry) *
    (1 - squeeze) *
    (1 - maniac) *
    (1 - silly)
  const sillyMouth = silly * clamp(sillyMouthShare, 0, 1)
  const symbolBlocker =
    (1 - dizzy) * (1 - squeeze) * (1 - cry) * (1 - silly) * (1 - lovestruck)
  if (layer.fade === 'eyeDizzy') return dizzy
  if (layer.fade === 'eyeCry') return cry * (1 - dizzy)
  if (layer.fade === 'eyeSilly') return silly
  if (layer.fade === 'lovestruckHeart') {
    const open = layer.side === 'L' ? driver.eyeOpenL : driver.eyeOpenR
    return lovestruck * smoothstep((open - 0.12) / 0.28)
  }
  if (layer.fade === 'lovestruckFace') return lovestruck
  if (layer.fade === 'lovestruckDrool') return lovestruck
  if (layer.fade === 'maniacEyeShadow') return maniac * symbolBlocker
  if (layer.fade === 'maniacMouthShadow') return maniac * symbolBlocker
  if (layer.fade === 'angerMark') return anger * (1 - maniac) * symbolBlocker
  if (layer.fade === 'speechlessSweat') {
    return speechless * (1 - anger) * (1 - maniac) * symbolBlocker
  }
  if (layer.fade === 'accentNotes') return smoothstep(driver.accentNotes)
  if (layer.fade === 'accentQuestion') return smoothstep(driver.accentQuestion)
  if (layer.fade === 'accentSleep') return smoothstep(driver.accentSleep)
  if (layer.fade === 'accentSparkle') return smoothstep(driver.accentSparkle)
  if (layer.fade === 'accentBlush') return smoothstep(driver.accentBlush)
  if (layer.fade === 'mouthCry') return arriving(mouthCry)
  if (layer.fade === 'mouthSilly') return arriving(sillyMouth) * (1 - mouthCry)
  if (layer.fade === 'eyeSqueeze') {
    return squeeze * (1 - dizzy) * (1 - cry)
  }
  if (layer.fade === 'eyeOpen' || layer.fade === 'eyeClose') {
    const open = layer.side === 'L' ? driver.eyeOpenL : driver.eyeOpenR
    const faded = openEyeShare(open, driver.eyeEase)
    return (
      (layer.fade === 'eyeOpen' ? faded : 1 - faded) *
      (1 - dizzy) *
      (1 - squeeze) *
      (1 - cry) *
      (1 - silly)
    )
  }
  if (
    layer.fade === 'mouthOpen' ||
    layer.fade === 'mouthWide' ||
    layer.fade === 'mouthRound' ||
    layer.fade === 'mouthNarrow' ||
    layer.fade === 'mouthClose' ||
    layer.fade === 'mouthManiac'
  ) {
    return (
      mouthLayerMix(
        layer.fade,
        maniac,
        regularMouthMaterial(driver, activeMouthMaterial),
        closedLinger(driver),
        partingReveal(driver),
      ) *
      covered(mouthCry) *
      covered(sillyMouth)
    )
  }
  return 1
}

export function createAnime25DOpacityFrame(): Anime25DOpacityFrame {
  return {
    dizzy: 0,
    cry: 0,
    mouthCry: 0,
    squeeze: 0,
    anger: 0,
    speechless: 0,
    accentNotes: 0,
    accentQuestion: 0,
    accentSleep: 0,
    accentSparkle: 0,
    accentBlush: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
    sillyMouth: 0,
    symbolBlocker: 1,
    eyeOpenL: 1,
    eyeOpenR: 1,
    lovestruckHeartL: 0,
    lovestruckHeartR: 0,
    activeMouthMaterial: 'mouthClose',
    mouthUnderlay: 'mouthClose',
    mouthPrevious: 'mouthClose',
    mouthHandoff: 1,
    continuousMouth: false,
    closedLinger: 0,
    partingReveal: 1,
  }
}

export function writeAnime25DOpacityFrame(
  output: Anime25DOpacityFrame,
  driver: Readonly<Anime25DDriver>,
  activeMouthMaterial: SpeechMouthMaterial,
  sillyMouthShare = 1,
  handoff?: Readonly<{ previous: SpeechMouthMaterial, handoff: number }>,
  continuousMouth = false,
): void {
  const dizzy = smoothstep(driver.eyeDizzy)
  const cry = smoothstep(driver.eyeCry)
  const squeeze = smoothstep(driver.eyeSqueeze)
  const anger = smoothstep(driver.anger)
  const speechless = smoothstep(driver.speechless)
  const maniac = smoothstep(driver.maniac)
  const silly =
    smoothstep(driver.silly) *
    (1 - dizzy) *
    (1 - cry) *
    (1 - squeeze) *
    (1 - maniac)
  const lovestruck =
    smoothstep(driver.lovestruck) *
    (1 - dizzy) *
    (1 - cry) *
    (1 - squeeze) *
    (1 - maniac) *
    (1 - silly)
  const sillyMouth = silly * clamp(sillyMouthShare, 0, 1)
  const symbolBlocker =
    (1 - dizzy) * (1 - squeeze) * (1 - cry) * (1 - silly) * (1 - lovestruck)
  const eyeOpenL = openEyeShare(driver.eyeOpenL, driver.eyeEase)
  const eyeOpenR = openEyeShare(driver.eyeOpenR, driver.eyeEase)
  output.dizzy = dizzy
  output.cry = cry
  output.mouthCry = cry * (1 - dizzy)
  output.squeeze = squeeze
  output.anger = anger
  output.speechless = speechless
  output.accentNotes = smoothstep(driver.accentNotes)
  output.accentQuestion = smoothstep(driver.accentQuestion)
  output.accentSleep = smoothstep(driver.accentSleep)
  output.accentSparkle = smoothstep(driver.accentSparkle)
  output.accentBlush = smoothstep(driver.accentBlush)
  output.maniac = maniac
  output.silly = silly
  output.lovestruck = lovestruck
  output.sillyMouth = sillyMouth
  output.symbolBlocker = symbolBlocker
  output.eyeOpenL = eyeOpenL
  output.eyeOpenR = eyeOpenR
  output.lovestruckHeartL =
    lovestruck * smoothstep((driver.eyeOpenL - 0.12) / 0.28)
  output.lovestruckHeartR =
    lovestruck * smoothstep((driver.eyeOpenR - 0.12) / 0.28)
  output.activeMouthMaterial = activeMouthMaterial
  const fold = (material: SpeechMouthMaterial) =>
    continuousMouth && isSpeakingMaterial(material) ? 'mouthOpen' : material
  output.mouthUnderlay = fold(regularMouthMaterial(driver, activeMouthMaterial))
  output.mouthPrevious = fold(handoff?.previous ?? output.mouthUnderlay)
  output.mouthHandoff = handoff?.handoff ?? 1
  output.continuousMouth = continuousMouth
  output.closedLinger = closedLinger(driver)
  output.partingReveal = partingReveal(driver)
}

export function fadeOpacityFromFrame(
  layer: Pick<Anime25DPlaybackLayer, 'fade' | 'side'>,
  frame: Readonly<Anime25DOpacityFrame>,
): number {
  const fade = layer.fade
  if (!fade) return 1
  if (fade === 'eyeDizzy') return frame.dizzy
  if (fade === 'eyeCry') return frame.cry * (1 - frame.dizzy)
  if (fade === 'eyeSilly') return frame.silly
  if (fade === 'lovestruckHeart') {
    return layer.side === 'L' ? frame.lovestruckHeartL : frame.lovestruckHeartR
  }
  if (fade === 'lovestruckFace' || fade === 'lovestruckDrool') {
    return frame.lovestruck
  }
  if (fade === 'maniacEyeShadow' || fade === 'maniacMouthShadow') {
    return frame.maniac * frame.symbolBlocker
  }
  if (fade === 'angerMark') {
    return frame.anger * (1 - frame.maniac) * frame.symbolBlocker
  }
  if (fade === 'speechlessSweat') {
    return (
      frame.speechless *
      (1 - frame.anger) *
      (1 - frame.maniac) *
      frame.symbolBlocker
    )
  }
  if (fade === 'accentNotes') return frame.accentNotes
  if (fade === 'accentQuestion') return frame.accentQuestion
  if (fade === 'accentSleep') return frame.accentSleep
  if (fade === 'accentSparkle') return frame.accentSparkle
  if (fade === 'accentBlush') return frame.accentBlush
  if (fade === 'mouthCry') return arriving(frame.mouthCry)
  if (fade === 'mouthSilly') {
    return arriving(frame.sillyMouth) * (1 - frame.mouthCry)
  }
  if (fade === 'eyeSqueeze') {
    return frame.squeeze * (1 - frame.dizzy) * (1 - frame.cry)
  }
  if (fade === 'eyeOpen' || fade === 'eyeClose') {
    const open = layer.side === 'L' ? frame.eyeOpenL : frame.eyeOpenR
    return (
      (fade === 'eyeOpen' ? open : 1 - open) *
      (1 - frame.dizzy) *
      (1 - frame.squeeze) *
      (1 - frame.cry) *
      (1 - frame.silly)
    )
  }
  if (
    fade === 'mouthOpen' ||
    fade === 'mouthWide' ||
    fade === 'mouthRound' ||
    fade === 'mouthNarrow' ||
    fade === 'mouthClose' ||
    fade === 'mouthManiac'
  ) {
    return (
      mouthLayerMix(
        fade,
        frame.maniac,
        frame.mouthUnderlay,
        frame.closedLinger,
        frame.partingReveal,
        frame.mouthPrevious,
        frame.mouthHandoff,
      ) *
      covered(frame.mouthCry) *
      covered(frame.sillyMouth)
    )
  }
  return 1
}

/** Invisible generated variants do not need CPU deformation or GPU uploads. */
export function shouldDeformLayer(
  layer: Pick<Anime25DPlaybackLayer, 'name'>,
  opacity: number,
): boolean {
  return opacity >= 0.004 || layer.name.startsWith('eyewhite')
}

function isSpeakingMaterial(material: SpeechMouthMaterial): boolean {
  return material === 'mouthOpen' || material === 'mouthWide' || material === 'mouthRound' || material === 'mouthNarrow'
}

function mouthLayerMix(
  fade: string,
  maniac: number,
  underlay: SpeechMouthMaterial,
  linger: number,
  reveal: number,
  previous: SpeechMouthMaterial = underlay,
  handoff = 1,
): number {
  if (fade === 'mouthManiac') return arriving(maniac)
  const under = covered(maniac)
  // One open drawing hands over to another: the new one comes in over the
  // old, then the old goes. The closed line has its own linger below.
  const handing = previous !== underlay && previous !== 'mouthClose' && previous !== 'mouthManiac' &&
    underlay !== 'mouthClose' && handoff < 1
  if (fade === underlay) {
    // A parting slit comes in over the lip line rather than cutting to it.
    const incoming = handing ? smoothstep(handoff * 2) : 1
    return (fade === 'mouthClose' ? 1 : reveal) * incoming * under
  }
  if (handing && fade === previous) return reveal * (1 - smoothstep(handoff * 2 - 1)) * under
  if (fade === 'mouthClose' && underlay !== 'mouthManiac') {
    return linger * under
  }
  return 0
}

/**
 * An expression's own mouth (crying, laughing, tongue out) is drawn over the
 * speaking one. It comes in over it, and the speaking mouth goes only once
 * it is covered, so the mouth is never half see-through between the two.
 */
function arriving(amount: number): number {
  return smoothstep(amount * 2)
}

function covered(amount: number): number {
  return 1 - smoothstep(amount * 2 - 1)
}

/** Opening over which the closed line fades out above a parting mouth. */
const CLOSED_LINGER_OPENING = 0.3
/** Opening over which the parting slit fades in over that line. */
const PARTING_REVEAL_OPENING = 0.08

function closedLinger(driver: Readonly<Anime25DDriver>): number {
  return 1 - smoothstep(mouthOpenMix(driver) / CLOSED_LINGER_OPENING)
}

function partingReveal(driver: Readonly<Anime25DDriver>): number {
  return smoothstep(mouthOpenMix(driver) / PARTING_REVEAL_OPENING)
}

/**
 * How much of the open eye shows at `open`, the drawn closed eye taking the
 * rest. The lids close over the open drawing all the way down to a slit, and
 * only then does the closed drawing take over, in a short band: a held squint
 * (the director's clear tier is 0.35) is the open eye alone, never the two
 * drawings half through each other. `ease` (how readily the eyes close) moves
 * the band up.
 */
export function openEyeShare(open: number, ease: number): number {
  return smoothstep((open - (0.05 + ease * 0.45)) / 0.07)
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
