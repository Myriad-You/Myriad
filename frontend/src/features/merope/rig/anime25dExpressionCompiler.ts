import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { RasterLayer } from './anime25dImportTypes'
import { resolveAnime25DFaceFrame } from '../expressionShapes/faceFrame'
import { synthesizeMissingExpressionSymbols, synthesizeMissingLovestruckEffects } from './expressionEffectLayers'
import { synthesizeMissingCryEyes, synthesizeMissingDizzyEyes, synthesizeMissingManiacEyeShadows, synthesizeMissingSillyEyes, synthesizeMissingSqueezeEyes } from './expressionEyeLayers'
import { synthesizeMissingMouthExpressions } from './expressionMouthLayers'

export function compileAnime25DExpressionLayers(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
): RasterLayer[] {
  const frame = resolveAnime25DFaceFrame(anchors)
  let output = synthesizeMissingDizzyEyes(layers, anchors, frame)
  output = synthesizeMissingSqueezeEyes(output, anchors, frame)
  output = synthesizeMissingCryEyes(output, anchors, frame)
  output = synthesizeMissingSillyEyes(output, anchors, frame)
  output = synthesizeMissingLovestruckEffects(output, anchors, frame)
  output = synthesizeMissingManiacEyeShadows(output, anchors, frame)
  output = synthesizeMissingMouthExpressions(output, anchors, frame)
  return synthesizeMissingExpressionSymbols(output, anchors, frame)
}
