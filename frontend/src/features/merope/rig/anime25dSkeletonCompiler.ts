import type { Anime25DLayerRole } from './anime25d'
import type { AnimeAnchors, PreparedLayer } from './anime25dImportTypes'
import type {
  RigBone,
  RigBoneHandle,
  RigLayerMeshSource,
  RigLayerSource,
  RigPoint,
  RigRect,
} from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import { ANIME25D_LAYER_DEPTH } from './anime25d'
import { MAX_RIG_BONES } from './contract'

export function buildAnime25DBonesAndHandles(
  layers: PreparedLayer[],
  anchors: AnimeAnchors,
): {
  bones: RigBone[]
  layerHandles: Map<string, RigBoneHandle[]>
  secondaryBoneIds: string[]
} {
  const bones: RigBone[] = [
    { id: 'root', parent: null, pivot: anchors.bodyBottom },
    { id: 'body', parent: 'root', pivot: anchors.neck },
    { id: 'head', parent: 'body', pivot: anchors.neck },
    { id: 'face', parent: 'head', pivot: anchors.faceCenter },
  ]
  const ensureBone = (id: string, parent: string, pivot: RigPoint): string => {
    if (!bones.some((bone) => bone.id === id)) bones.push({ id, parent, pivot })
    return id
  }
  for (const side of ['left', 'right'] as const) {
    const eye = anchors.eyes[side]
    if (!eye) continue
    ensureBone(`${side}-eye`, 'face', eye)
    if (
      layers.some((layer) => layer.side === side && layer.role === 'eyewhite')
    ) {
      ensureBone(`a25d-eyewhite-${side}`, 'face', eye)
    }
    if (anchors.irises[side]) {
      ensureBone(`a25d-irides-${side}`, `${side}-eye`, anchors.irises[side]!)
    }
    if (
      layers.some(
        (layer) =>
          layer.side === side &&
          (layer.role === 'eyelash' || layer.role === 'eye-close'),
      )
    ) {
      ensureBone(`a25d-eyelash-${side}`, 'face', eye)
    }
    ensureBone(`a25d-eyebrow-${side}`, 'face', {
      x: eye.x,
      y: eye.y - anchors.face.height * 0.12,
    })
  }
  if (anchors.mouth) ensureBone('mouth', 'face', anchors.mouth)
  if (layers.some((layer) => layer.role === 'topwear')) {
    ensureBone('a25d-chest', 'body', {
      x: anchors.neck.x,
      y: anchors.neck.y + anchors.face.height * 0.35,
    })
  }
  if (layers.some((layer) => layer.role === 'handwear')) {
    const handwearLayers = layers.filter((layer) => layer.role === 'handwear')
    const handwear = unionLayerBounds(handwearLayers)
    ensureBone('a25d-handwear', 'body', {
      x: handwear.x + handwear.width / 2,
      y: handwear.y + handwear.height * 0.22,
    })
    for (const side of ['left', 'right'] as const) {
      const sideLayer = handwearLayers.find((layer) => layer.side === side)
      if (!sideLayer) continue
      ensureBone(`a25d-handwear-${side}`, 'a25d-handwear', {
        x: sideLayer.bounds.x + sideLayer.bounds.width / 2,
        y: sideLayer.bounds.y + sideLayer.bounds.height * 0.16,
      })
    }
  }
  const independentHeadRoles: Anime25DLayerRole[] = [
    'nose',
    'ears',
    'earwear',
    'headwear',
    'facedetail',
  ]
  for (const role of independentHeadRoles) {
    const layer = layers.find((candidate) => candidate.role === role)
    if (!layer) continue
    ensureBone(`a25d-${role}`, 'head', rectCenter(layer.bounds))
  }

  const secondaryBoneIds: string[] = []
  const hairBones = new Map<string, RigBoneHandle[]>()
  let availableStrands = Math.max(
    0,
    Math.floor((MAX_RIG_BONES - bones.length) / 2),
  )
  for (const layer of layers.filter(
    (candidate) =>
      candidate.role === 'front-hair' || candidate.role === 'back-hair',
  )) {
    const selected = layer.strands.slice(0, availableStrands)
    availableStrands -= selected.length
    const handles: RigBoneHandle[] = []
    const spacing = layer.bounds.width / Math.max(2, selected.length)
    selected.forEach((strand, index) => {
      const prefix = `a25d-${layer.id}-strand-${index + 1}`
      const root = `${prefix}-hair-root`
      const tip = `${prefix}-hair-tip`
      const midY = strand.rootY + (strand.tipY - strand.rootY) * 0.48
      bones.push({
        id: root,
        parent: 'head',
        pivot: { x: strand.x, y: strand.rootY },
      })
      bones.push({ id: tip, parent: root, pivot: { x: strand.x, y: midY } })
      secondaryBoneIds.push(root, tip)
      handles.push(
        {
          boneId: root,
          start: { x: strand.x, y: strand.rootY },
          end: { x: strand.x, y: midY },
          falloff: Math.max(0.025, spacing * 0.95),
        },
        {
          boneId: tip,
          start: { x: strand.x, y: midY },
          end: { x: strand.x, y: strand.tipY },
          falloff: Math.max(0.025, spacing * 0.82),
        },
      )
    })
    if (handles.length === 0) handles.push(fullLayerHandle(layer, 'head'))
    hairBones.set(layer.id, handles)
  }
  if (bones.length > MAX_RIG_BONES) {
    throw new Error(
      currentCopy().merope.anime25dBoneLimit.replace(
        '{max}',
        String(MAX_RIG_BONES),
      ),
    )
  }

  const layerHandles = new Map<string, RigBoneHandle[]>()
  for (const layer of layers) {
    const hair = hairBones.get(layer.id)
    if (hair) {
      layerHandles.set(layer.id, hair)
      continue
    }
    layerHandles.set(layer.id, handlesForLayer(layer, bones))
  }
  return { bones, layerHandles, secondaryBoneIds }
}

function handlesForLayer(
  layer: PreparedLayer,
  bones: RigBone[],
): RigBoneHandle[] {
  const has = (id: string) => bones.some((bone) => bone.id === id)
  const side = layer.side
  if (layer.role === 'face') return [fullLayerHandle(layer, 'face')]
  if (
    layer.role === 'maniac-eye-shadow' ||
    layer.role === 'maniac-mouth-shadow' ||
    layer.role === 'anger-mark' ||
    layer.role === 'speechless-sweat' ||
    layer.role === 'lovestruck-face-effect' ||
    layer.role === 'lovestruck-drool'
  ) {
    return [fullLayerHandle(layer, 'face')]
  }
  if (side && layer.role === 'eyewhite' && has(`a25d-eyewhite-${side}`)) {
    return [fullLayerHandle(layer, `a25d-eyewhite-${side}`)]
  }
  if (
    side &&
    (layer.role === 'eyelash' ||
      layer.role === 'eye-close' ||
      layer.role === 'eye-dizzy' ||
      layer.role === 'eye-squeeze' ||
      layer.role === 'eye-cry' ||
      layer.role === 'eye-silly-white') &&
    has(`a25d-eyelash-${side}`)
  ) {
    return [fullLayerHandle(layer, `a25d-eyelash-${side}`)]
  }
  if (
    side &&
    (layer.role === 'irides' ||
      layer.role === 'iris-silly' ||
      layer.role === 'lovestruck-heart') &&
    has(`a25d-irides-${side}`)
  ) {
    return [fullLayerHandle(layer, `a25d-irides-${side}`)]
  }
  if (side && layer.role === 'eyebrow' && has(`a25d-eyebrow-${side}`)) {
    return [fullLayerHandle(layer, `a25d-eyebrow-${side}`)]
  }
  if (
    (layer.role === 'mouth-open' ||
      layer.role === 'mouth-wide' ||
      layer.role === 'mouth-round' ||
      layer.role === 'mouth-narrow' ||
      layer.role === 'mouth-close' ||
      layer.role === 'mouth-cry' ||
      layer.role === 'mouth-maniac' ||
      layer.role === 'mouth-silly') &&
    has('mouth')
  ) {
    return [fullLayerHandle(layer, 'mouth')]
  }
  if (layer.role === 'topwear' && has('a25d-chest')) {
    return verticalBlendHandles(layer, 'body', 'a25d-chest', 0.58)
  }
  if (layer.role === 'collar-back' || layer.role === 'collar-front') {
    return [fullLayerHandle(layer, 'body')]
  }
  if (layer.role === 'neck') {
    return verticalBlendHandles(layer, 'head', 'body', 0.72)
  }
  if (layer.role === 'handwear' && has('a25d-handwear')) {
    const sideBone = layer.side ? `a25d-handwear-${layer.side}` : ''
    return [
      fullLayerHandle(
        layer,
        sideBone && has(sideBone) ? sideBone : 'a25d-handwear',
      ),
    ]
  }
  if (layer.role === 'bottomwear') return [fullLayerHandle(layer, 'root')]
  if (['neckwear', 'wings', 'tail'].includes(layer.role)) {
    return [fullLayerHandle(layer, 'body')]
  }
  if (layer.role === 'eyewear') return [fullLayerHandle(layer, 'head')]
  const dedicated = `a25d-${layer.role}`
  if (has(dedicated)) return [fullLayerHandle(layer, dedicated)]
  const group =
    layer.bounds.y + layer.bounds.height / 2 < 0.62 ? 'head' : 'body'
  return [fullLayerHandle(layer, group)]
}

function fullLayerHandle(layer: PreparedLayer, boneId: string): RigBoneHandle {
  return {
    boneId,
    start: {
      x: layer.bounds.x + layer.bounds.width / 2,
      y: layer.bounds.y,
    },
    end: {
      x: layer.bounds.x + layer.bounds.width / 2,
      y: layer.bounds.y + layer.bounds.height,
    },
    falloff: Math.max(layer.bounds.width, layer.bounds.height, 0.02),
  }
}

function verticalBlendHandles(
  layer: PreparedLayer,
  topBone: string,
  bottomBone: string,
  split: number,
): RigBoneHandle[] {
  const centerX = layer.bounds.x + layer.bounds.width / 2
  const middleY = layer.bounds.y + layer.bounds.height * split
  const falloff = Math.max(
    layer.bounds.width * 0.68,
    layer.bounds.height * 0.38,
  )
  return [
    {
      boneId: topBone,
      start: { x: centerX, y: layer.bounds.y },
      end: { x: centerX, y: middleY },
      falloff,
    },
    {
      boneId: bottomBone,
      start: { x: centerX, y: middleY },
      end: { x: centerX, y: layer.bounds.y + layer.bounds.height },
      falloff,
    },
  ]
}

export function buildAnime25DLayerSources(
  layers: PreparedLayer[],
  handles: Map<string, RigBoneHandle[]>,
): RigLayerSource[] {
  return layers.map((layer) => {
    const mesh = gridMesh(layer.bounds, layer.role)
    const depth =
      layer.role === 'unknown' ? 1 : ANIME25D_LAYER_DEPTH[layer.role]
    return {
      id: `a25d-${layer.id}`,
      textureId: 'atlas',
      textureBounds: layer.textureBounds,
      // ag-psd exposes this PSD bottom-to-top. Anime2.5DRig deliberately
      // overrides that order with its semantic depth table, then keeps the PSD
      // order as a stable tie-break for numbered/repeated layers.
      zIndex: Math.round(depth * 100) * 100 + Math.round(layer.order),
      opacity: 1,
      slot: layer.slot,
      variant: layer.variant,
      contours: [],
      mesh,
      boneHandles: handles.get(layer.id) || [fullLayerHandle(layer, 'body')],
    }
  })
}

export function gridMesh(
  bounds: RigRect,
  role: Anime25DLayerRole | 'unknown',
): RigLayerMeshSource {
  const deformable =
    role === 'front-hair' ||
    role === 'back-hair' ||
    role === 'topwear' ||
    role === 'neck'
  const cell = deformable ? 0.035 : 0.075
  const columns = clampInt(
    Math.ceil(bounds.width / cell),
    2,
    deformable ? 14 : 8,
  )
  const rows = clampInt(Math.ceil(bounds.height / cell), 2, deformable ? 18 : 8)
  const vertices: RigPoint[] = []
  const indices: number[] = []
  for (let row = 0; row <= rows; row += 1) {
    for (let column = 0; column <= columns; column += 1) {
      vertices.push({
        x: bounds.x + (bounds.width * column) / columns,
        y: bounds.y + (bounds.height * row) / rows,
      })
    }
  }
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const topLeft = row * (columns + 1) + column
      const topRight = topLeft + 1
      const bottomLeft = topLeft + columns + 1
      const bottomRight = bottomLeft + 1
      indices.push(
        topLeft,
        topRight,
        bottomLeft,
        topRight,
        bottomRight,
        bottomLeft,
      )
    }
  }
  return { vertices, indices }
}

function unionLayerBounds(layers: PreparedLayer[]): RigRect {
  const x0 = Math.min(...layers.map((layer) => layer.bounds.x))
  const y0 = Math.min(...layers.map((layer) => layer.bounds.y))
  const x1 = Math.max(
    ...layers.map((layer) => layer.bounds.x + layer.bounds.width),
  )
  const y1 = Math.max(
    ...layers.map((layer) => layer.bounds.y + layer.bounds.height),
  )
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

function rectCenter(rect: RigRect): RigPoint {
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, Math.round(value)))
}
