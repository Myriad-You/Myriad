import type { Psd } from 'ag-psd'
import type { Anime25DImportCopy } from './anime25dImportCopy'
import type { Anime25DSourceReference } from './anime25dImportTypes'
import type { AuthoredExpressionReference } from './authoredExpression'
import type { CharacterAssetProfile } from './contract'
import type { MotionExposureFinding } from './motionExposure'
import type { Anime25DPsdReconciliation } from './psdReconciliation'
import type { MeropeRigImportSource } from './types'
import { analyzeAnime25DMouthProfile } from '../anime25drig/mouthProfile'
import { buildAnime25DPlayback, remapRiggerAnchors } from '../anime25drig/playback'
import { rigger as Rigger } from '../anime25drig/upstream/rigger'
import { estimateAnime25DMouthAnchor, resolveAnime25DFaceFrame } from '../expressionShapes/faceFrame'
import { validateAnime25DCharacterLayers } from './anime25dAssetValidation'
import { packAnime25DAtlas, visibleInAnalysisReference } from './anime25dAtlasCompiler'
import { splitHighCollarOcclusion } from './anime25dCollarCompiler'
import { compileAnime25DExpressionLayers } from './anime25dExpressionCompiler'
import { anime25DImportProfile } from './anime25dImportProfile'
import { anime25DBaseRole, normalizeAnime25DLayerName } from './anime25dLayerSemantics'
import { buildAnime25DBonesAndHandles, buildAnime25DLayerSources } from './anime25dSkeletonCompiler'
import { addAuthoredExpressionLayers } from './authoredExpression'
import { compensateSyntheticClosedEyeAngles } from './closedEyeCompensation'
import { CHARACTER_ASSET_PROFILES, MAX_RIG_PARTS, RIG_IR_VERSION } from './contract'
import { formatTemplate } from './formatTemplate'
import { contentFrame, deriveAnchors, semanticAnchors, standingStance } from './importerAnchors'
import { assignCrossfadeSlots, hasStaticSeeThroughMouth, preserveStaticMouthAsClosed, splitHandwearIfNeeded, splitLowerLimbsIfNeeded, splitVariantEyesIfNeeded } from './importerLayerSplits'
import { addHiddenArmFragments, anime25DShoulderSeeds } from './linkedHandwear'
import { findMotionExposure } from './motionExposure'
import { inferOutfitProfileFromPartIds } from './outfit'
import { repairAnime25DPsd } from './psdRepair'
import { flattenPsdForRigger, flattenVisibleLayers, genericCloseParts, rasterFromRiggerPart } from './riggerBridge'

export { ANIME25D_LAYER_DEPTH, type Anime25DLayerRole } from './anime25d'
export type { Anime25DSourceReference } from './anime25dImportTypes'

export interface PreparedAnime25DRigImport {
  atlas: Blob
  analysisReference: Blob
  source: MeropeRigImportSource
  partCount: number
  /** Diagnosis against the source illustration; absent without one. */
  reconciliation: Anime25DPsdReconciliation | null
  /** Holes a blink or a spoken vowel would open in the face. */
  motionExposure: MotionExposureFinding[]
}
export {
  anime25DBaseRole,
  normalizeAnime25DLayerName,
} from './anime25dLayerSemantics'

export function isAnime25DDocument(psd: Psd): boolean {
  const names = flattenVisibleLayers(psd.children ?? []).map((layer) =>
    normalizeAnime25DLayerName(layer.name),
  )
  return names.some((name) => anime25DBaseRole(name) === 'face')
}

export async function prepareAnime25DRigPsd(
  psd: Psd,
  sourceMasterAssetId: string,
  copy: Anime25DImportCopy,
  onStage?: (stage: 'validated' | 'packing') => void,
  sourceGenerationFingerprint?: string,
  sourceReference?: Anime25DSourceReference,
  expressionReferences: readonly AuthoredExpressionReference[] = [],
  profile: CharacterAssetProfile = 'bust',
): Promise<PreparedAnime25DRigImport> {
  if (!isAnime25DDocument(psd)) {
    throw new Error(copy.anime25dMissingFace)
  }
  const importProfile = anime25DImportProfile(profile)
  const staticSeeThroughMouth = hasStaticSeeThroughMouth(psd)
  const working = flattenPsdForRigger(psd, importProfile)
  Rigger.cleanPsdLayers(working)
  // A named but empty/hidden face must never silently acquire guessed pivots.
  if (
    !working.children?.some(
      (layer) =>
        Rigger.baseName(layer.name ?? '') === 'face' &&
        layer.imageData?.data.some(
          (value, index) => index % 4 === 3 && value > 8,
        ),
    )
  ) {
    throw new Error(copy.anime25dMissingFace)
  }
  const rig = Rigger.buildRig(working, { generic: genericCloseParts() })
  compensateSyntheticClosedEyeAngles(rig.layers)
  onStage?.('validated')
  const usedIds = new Set<string>()
  let layers = rig.layers.map((part) => rasterFromRiggerPart(part, usedIds))
  if (staticSeeThroughMouth) layers = preserveStaticMouthAsClosed(layers)
  layers = splitHandwearIfNeeded(layers, rig.anchors.face.cx)
  layers = splitLowerLimbsIfNeeded(layers)
  layers = addHiddenArmFragments(layers, anime25DShoulderSeeds(layers), new Set(layers.map((layer) => layer.id)))
  layers = splitVariantEyesIfNeeded(layers, rig.anchors.face.cx, 'eye-dizzy')
  layers = splitVariantEyesIfNeeded(layers, rig.anchors.face.cx, 'eye-squeeze')
  layers = splitVariantEyesIfNeeded(layers, rig.anchors.face.cx, 'eye-cry')
  if (
    !layers.some(
      (layer) => layer.role === 'mouth-open' || layer.role === 'mouth-close',
    )
  ) {
    // The frozen rigger guesses a fixed-pixel box off the face centroid.
    const frame = resolveAnime25DFaceFrame(rig.anchors)
    if (frame.landmarks) {
      rig.anchors.mouth = estimateAnime25DMouthAnchor(frame, rig.anchors.face)
    }
  }
  layers = addAuthoredExpressionLayers(
    layers,
    rig.anchors,
    sourceReference,
    expressionReferences,
  )
  layers = compileAnime25DExpressionLayers(layers, rig.anchors)
  layers = splitHighCollarOcclusion(layers, rig.anchors, sourceReference)
  layers.forEach((layer, index) => {
    layer.order = index
  })
  assignCrossfadeSlots(layers)
  validateAnime25DCharacterLayers(layers, copy)
  let reconciliation: Anime25DPsdReconciliation | null = null
  if (sourceReference) {
    const repair = repairAnime25DPsd(
      layers,
      visibleInAnalysisReference,
      sourceReference,
      Math.max(0, MAX_RIG_PARTS - layers.length),
    )
    layers = repair.layers
    reconciliation = repair.reconciliation
    layers.forEach((layer, index) => {
      layer.order = index
    })
  }
  const motionExposure = findMotionExposure(layers)
  const faceCenter = {
    x: rig.anchors.face.cx,
    y: rig.anchors.face.cy,
  }
  if (layers.length === 0 || layers.length > MAX_RIG_PARTS) {
    throw new Error(
      formatTemplate(copy.anime25dPartCount, { max: MAX_RIG_PARTS }),
    )
  }
  const frame = contentFrame(psd, layers, importProfile)
  const stance = importProfile.bodyPivot === 'hips' ? standingStance(layers) : null
  if (importProfile.bodyPivot === 'hips' && !stance) {
    throw new Error(formatTemplate(copy.anime25dMissingLayer, { role: 'legwear' }))
  }
  onStage?.('packing')
  const {
    atlas,
    analysisReference,
    layers: prepared,
    width: packedWidth,
    height: packedHeight,
  } = await packAnime25DAtlas(frame, layers, copy)
  const anchors = deriveAnchors(frame, prepared, faceCenter, copy)
  const { bones, layerHandles, secondaryBoneIds } =
    buildAnime25DBonesAndHandles(prepared, anchors, copy)
  const rigLayers = buildAnime25DLayerSources(prepared, layerHandles)
  const partIds = prepared.map((layer) => `a25d-${layer.id}`)
  const playbackAnchors = remapRiggerAnchors(rig.anchors, frame, stance)
  const mouthProfile = analyzeAnime25DMouthProfile(
    prepared,
    frame,
    playbackAnchors.mouth,
  )
  const anime25dPlayback = buildAnime25DPlayback(
    {
      frameWidth: frame.width,
      frameHeight: frame.height,
      layers: prepared,
      anchors: playbackAnchors,
      mouthProfile,
    },
    copy,
  )
  const outfitProfile = inferOutfitProfileFromPartIds(partIds)
  const semanticBones: Record<string, string> = {
    root: 'root',
    torso: 'body',
    head: 'head',
    face: 'face',
  }
  if (bones.some((bone) => bone.id === 'left-eye')) {
    semanticBones['left-eye'] = 'left-eye'
  }
  if (bones.some((bone) => bone.id === 'right-eye')) {
    semanticBones['right-eye'] = 'right-eye'
  }
  if (bones.some((bone) => bone.id === 'mouth')) semanticBones.mouth = 'mouth'
  if (bones.some((bone) => bone.id === 'a25d-handwear')) {
    semanticBones.handwear = 'a25d-handwear'
  }
  return {
    atlas,
    analysisReference,
    partCount: prepared.length,
    reconciliation,
    motionExposure,
    source: {
      rigIrVersion: RIG_IR_VERSION,
      characterAssetContractVersion: CHARACTER_ASSET_PROFILES[profile].contractVersion,
      ...(profile === 'bust' ? {} : { profile }),
      sourceMasterAssetId,
      ...(sourceGenerationFingerprint ? { sourceGenerationFingerprint } : {}),
      canvas: { ...importProfile.canvas },
      atlas: { id: 'atlas', width: packedWidth, height: packedHeight },
      bones,
      layers: rigLayers,
      outfitProfile,
      semanticAnchors: semanticAnchors(anchors),
      semantics: {
        bones: semanticBones,
        chains: { torso: ['root', 'body', 'head'] },
        secondaryBoneIds,
      },
      anime25dPlayback,
    },
  }
}
