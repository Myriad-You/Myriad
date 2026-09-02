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
}

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
}

/** Resolves stable mouth artwork references once instead of scanning per frame. */
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
  if (maniac > 0) {
    // An extreme mouth must still fit the character's lower face.
    // Blend the guard with the expression so entry/exit remains continuous.
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
  output.centerX += transition.centerOffsetX
  output.centerY += transition.centerOffsetY
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
  // Silly is a complete eye and mouth replacement, so it yields to every
  // artwork state that also replaces the eyes and to the wilder maniac face.
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
  // The vacant eyes and the vacant mouth are owned separately: speech keeps
  // the articulating mouth while the stare stays on the face.
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
  if (layer.fade === 'mouthCry') return mouthCry
  if (layer.fade === 'mouthSilly') return sillyMouth * (1 - mouthCry)
  if (layer.fade === 'eyeSqueeze') {
    return squeeze * (1 - dizzy) * (1 - cry)
  }
  if (layer.fade === 'eyeOpen' || layer.fade === 'eyeClose') {
    const open = layer.side === 'L' ? driver.eyeOpenL : driver.eyeOpenR
    const faded = smoothstep((open - (0.1 + driver.eyeEase * 0.45)) / 0.15)
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
      ) *
      (1 - mouthCry) *
      (1 - sillyMouth)
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
  }
}

/** Computes shared expression weights once for every layer in a frame. */
export function writeAnime25DOpacityFrame(
  output: Anime25DOpacityFrame,
  driver: Readonly<Anime25DDriver>,
  activeMouthMaterial: SpeechMouthMaterial,
  sillyMouthShare = 1,
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
  const eyeOpenL = smoothstep(
    (driver.eyeOpenL - (0.1 + driver.eyeEase * 0.45)) / 0.15,
  )
  const eyeOpenR = smoothstep(
    (driver.eyeOpenR - (0.1 + driver.eyeEase * 0.45)) / 0.15,
  )
  output.dizzy = dizzy
  output.cry = cry
  output.mouthCry = cry * (1 - dizzy)
  output.squeeze = squeeze
  output.anger = anger
  output.speechless = speechless
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
  output.mouthUnderlay = regularMouthMaterial(driver, activeMouthMaterial)
}

/** Resolves one preclassified layer from the shared frame weights. */
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
  if (fade === 'mouthCry') return frame.mouthCry
  if (fade === 'mouthSilly') {
    return frame.sillyMouth * (1 - frame.mouthCry)
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
      mouthLayerMix(fade, frame.maniac, frame.mouthUnderlay) *
      (1 - frame.mouthCry) *
      (1 - frame.sillyMouth)
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

function mouthLayerMix(
  fade: string,
  maniac: number,
  underlay: SpeechMouthMaterial,
): number {
  if (fade === 'mouthManiac') return maniac
  return fade === underlay ? 1 - maniac : 0
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
