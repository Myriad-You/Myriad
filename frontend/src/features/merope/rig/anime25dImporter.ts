import type { Psd } from 'ag-psd'
import type { Anime25DImportCopy } from './anime25dImportCopy'
import type { Anime25DSourceReference } from './anime25dImportTypes'
import type { AuthoredExpressionReference } from './authoredExpression'
import type { CharacterAssetProfile } from './contract'
import type { MotionExposureFinding } from './motionExposure'
import type { Anime25DPsdReconciliation } from './psdReconciliation'
import type { PsdSkeleton } from './skeleton'
import type { MeropeRigImportSource } from './types'
import { analyzeAnime25DMouthProfile } from '../anime25drig/mouthProfile'
import { buildAnime25DPlayback, remapRiggerAnchors } from '../anime25drig/playback'
import { rigger as Rigger } from '../anime25drig/upstream/rigger'
import { estimateAnime25DMouthAnchor, resolveAnime25DFaceFrame } from '../expressionShapes/faceFrame'
import { paintAccessoriesFromReference } from './accessoryPaint'
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
import { raiseEarwearOverFrontHair } from './earwearOrder'
import { formatTemplate } from './formatTemplate'
import { contentFrame, deriveAnchors, semanticAnchors, standingStance } from './importerAnchors'
import { assignCrossfadeSlots, hasStaticSeeThroughMouth, mirrorLostEyeWhite, preserveStaticMouthAsClosed, splitHandwearIfNeeded, splitLowerLimbsIfNeeded, splitVariantEyesIfNeeded, stackArmsByReference, stackHeadwearByReference, stackLowerLimbsByReference, stackNeckwearByReference, stackOpenEyesInOrder } from './importerLayerSplits'
import { addHiddenArmFragments, anime25DShoulderSeeds } from './linkedHandwear'
import { findMotionExposure } from './motionExposure'
import { inferOutfitProfileFromPartIds } from './outfit'
import { repairAnime25DPsd } from './psdRepair'
import { flattenPsdForRigger, flattenVisibleLayers, genericCloseParts, rasterFromRiggerPart } from './riggerBridge'
import { skeletonInFrame } from './skeleton'

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
  /** The document and where the playback frame cuts it, for keys measured on the document. */
  documentFrame: { width: number; height: number; x: number; y: number }
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
  skeleton?: PsdSkeleton,
): Promise<PreparedAnime25DRigImport> {
  if (!isAnime25DDocument(psd)) {
    throw new Error(copy.anime25dMissingFace)
  }
  const importProfile = anime25DImportProfile(profile)
  const staticSeeThroughMouth = hasStaticSeeThroughMouth(psd)
  const working = flattenPsdForRigger(psd, importProfile)
  Rigger.cleanPsdLayers(working)
  mirrorLostEyeWhite(working, Rigger.baseName)
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
  let layers = stackOpenEyesInOrder(rig.layers.map((part) => rasterFromRiggerPart(part, usedIds)))
  if (staticSeeThroughMouth) layers = preserveStaticMouthAsClosed(layers)
  layers = splitHandwearIfNeeded(layers, rig.anchors.face.cx)
  layers = splitLowerLimbsIfNeeded(layers)
  if (sourceReference) {
    layers = stackLowerLimbsByReference(layers, sourceReference)
    layers = stackNeckwearByReference(layers, sourceReference)
    layers = stackHeadwearByReference(layers, sourceReference)
    // A bust's arms are cut off above where a skirt would be.
    if (profile === 'fullBody') layers = stackArmsByReference(layers, sourceReference)
  }
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
    layers = paintAccessoriesFromReference(
      raiseEarwearOverFrontHair(repair.layers),
      visibleInAnalysisReference,
      sourceReference,
    )
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
  const joints = skeleton ? skeletonInFrame(skeleton, frame) : null
  if (joints) playbackAnchors.skeleton = joints
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
    documentFrame: { width: psd.width, height: psd.height, x: frame.x, y: frame.y },
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
