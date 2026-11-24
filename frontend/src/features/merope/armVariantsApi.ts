import type { ArmVariantGesture, ArmVariantSource } from '@myriad/figure/rig'
import { ARM_VARIANT_GESTURES } from '@myriad/figure/rig'
import { currentCopy } from '../../i18n/localeCopy'
import { apiService } from '../../services/api'
import { fullBodyPath, keptFailure, MeropeApiError, meropeError, PREFIX } from './api'

export type ArmVariantSide = ArmVariantSource['side']
/** A gesture on one arm, as the server names it: `wave-left`. */
export type ArmVariantKey = `${ArmVariantGesture}-${ArmVariantSide}`
export const ARM_VARIANT_KEYS: readonly ArmVariantKey[] = ARM_VARIANT_GESTURES.flatMap((gesture) =>
  (['left', 'right'] as const).map((side) => `${gesture}-${side}` as const),
)

/** Where a picture's gesture arms are kept: the worn bust's, or a full-body set's (`outfitId`). */
function armVariantsPath(outfitId?: string | null): string {
  return `${PREFIX}${outfitId ? fullBodyPath(outfitId) : ''}/arm-variants`
}

export interface SiteArmVariants {
  variants: ArmVariantSource[]
  /** Being drawn now: begun by this page, before a reload, or in another tab. */
  drawing: ArmVariantKey[]
  /** The ones whose last drawing was not kept, each with what went wrong as an error a request throws. */
  failed: Partial<Record<ArmVariantKey, MeropeApiError>>
}

/**
 * Gesture arms drawn for the current master portrait (a full-body set's
 * picture with `outfitId`), ready to cut into the rig at import, the ones
 * being drawn now, and the ones that could not be drawn.
 */
export async function listSiteArmVariants(outfitId?: string | null): Promise<SiteArmVariants> {
  try {
    const data = await apiService.get<{ variants?: Record<string, unknown>; drawing?: unknown; failed?: Record<string, unknown> }>(
      armVariantsPath(outfitId),
    )
    const drawing = Array.isArray(data.drawing) ? data.drawing : []
    const failed: SiteArmVariants['failed'] = {}
    for (const key of ARM_VARIANT_KEYS) {
      const failure = keptFailure(data.failed?.[key], currentCopy().merope.armVariantsFailed)
      if (failure) failed[key] = failure
    }
    return {
      variants: ARM_VARIANT_KEYS.flatMap((key) => {
        const variant = readArmVariant(key, data.variants?.[key])
        return variant ? [variant] : []
      }),
      drawing: ARM_VARIANT_KEYS.filter((key) => drawing.includes(key)),
      failed,
    }
  } catch (reason) {
    throw meropeError(reason, currentCopy().merope.armVariantsFailed)
  }
}

/**
 * Asks for one gesture arm on the current master portrait (a full-body set's picture with `outfitId`). Resolves
 * once the server has taken it up: it draws on its own, and the list tells when it is kept or why it was not.
 */
export async function startSiteArmVariant(key: ArmVariantKey, outfitId?: string | null): Promise<void> {
  try {
    await apiService.post<unknown>(`${armVariantsPath(outfitId)}/${key}`, {})
  } catch (reason) {
    throw meropeError(reason, currentCopy().merope.armVariantsFailed)
  }
}

/** A drawing the server took up and then neither kept nor said why not: it was restarted meanwhile. */
export function armVariantLost(): MeropeApiError {
  return new MeropeApiError(currentCopy().merope.armVariantsFailed, 502)
}

function readArmVariant(key: ArmVariantKey, value: unknown): ArmVariantSource | null {
  if (!value || typeof value !== 'object') return null
  const { url, hand, keypoints } = value as { url?: unknown; hand?: unknown; keypoints?: unknown }
  if (typeof url !== 'string' || !url.trim()) return null
  const [gesture, side] = key.split('-') as [ArmVariantGesture, ArmVariantSide]
  // Where the pose model found the raised hand, in portrait pixels; the cut seeds there.
  const points = Array.isArray(hand)
    ? hand.flatMap((point) =>
        Array.isArray(point) && typeof point[0] === 'number' && typeof point[1] === 'number'
          ? [[point[0], point[1]] as const]
          : [],
      )
    : []
  // Every joint it found on the drawing, with its score: the arm is measured as a 3D arm from them. Kept whole or
  // not at all (they are looked up by index); drawings kept before that have none.
  const joints = Array.isArray(keypoints)
    && keypoints.every((point) => Array.isArray(point) && point.length === 3 && point.every((v) => typeof v === 'number'))
    ? (keypoints as Array<[number, number, number]>).map(([x, y, score]) => [x, y, score] as const)
    : []
  return {
    gesture,
    side,
    url,
    ...(points.length > 0 ? { hand: points } : {}),
    ...(joints.length > 0 ? { keypoints: joints } : {}),
  }
}
