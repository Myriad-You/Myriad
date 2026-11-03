import type { ArmVariantGesture, ArmVariantSource } from '@myriad/figure/rig'
import { ARM_VARIANT_GESTURES } from '@myriad/figure/rig'
import { currentCopy } from '../../i18n/localeCopy'
import { apiService } from '../../services/api'
import { MeropeApiError, meropeError, PREFIX } from './api'

/** A drawing takes as long as a portrait drawn from scratch, and is checked after. */
const ARM_VARIANT_TIMEOUT_MS = 15 * 60 * 1000

export type ArmVariantSide = ArmVariantSource['side']
/** A gesture on one arm, as the server names it: `wave-left`. */
export type ArmVariantKey = `${ArmVariantGesture}-${ArmVariantSide}`
export const ARM_VARIANT_KEYS: readonly ArmVariantKey[] = ARM_VARIANT_GESTURES.flatMap((gesture) =>
  (['left', 'right'] as const).map((side) => `${gesture}-${side}` as const),
)

/**
 * Gesture arms drawn for the current master portrait, ready to cut into the
 * rig at import, and the ones being drawn now (begun before a reload, or in another tab).
 */
export async function listSiteArmVariants(): Promise<{ variants: ArmVariantSource[]; drawing: ArmVariantKey[] }> {
  try {
    const data = await apiService.get<{ variants?: Record<string, unknown>; drawing?: unknown }>(
      `${PREFIX}/arm-variants`,
    )
    const drawing = Array.isArray(data.drawing) ? data.drawing : []
    return {
      variants: ARM_VARIANT_KEYS.flatMap((key) => {
        const variant = readArmVariant(key, data.variants?.[key])
        return variant ? [variant] : []
      }),
      drawing: ARM_VARIANT_KEYS.filter((key) => drawing.includes(key)),
    }
  } catch (reason) {
    throw meropeError(reason, currentCopy().merope.armVariantsFailed)
  }
}

/** Draws one gesture arm on the current master portrait; resolves once it is kept. */
export async function generateSiteArmVariant(key: ArmVariantKey): Promise<ArmVariantSource> {
  try {
    const data = await apiService.post<Record<string, unknown>>(
      `${PREFIX}/arm-variants/${key}`,
      {},
      { timeout: ARM_VARIANT_TIMEOUT_MS },
    )
    const variant = readArmVariant(key, data)
    if (!variant) throw new MeropeApiError(currentCopy().merope.armVariantsFailed, 502)
    return variant
  } catch (reason) {
    if (reason instanceof MeropeApiError) throw reason
    throw meropeError(reason, currentCopy().merope.armVariantsFailed)
  }
}

function readArmVariant(key: ArmVariantKey, value: unknown): ArmVariantSource | null {
  if (!value || typeof value !== 'object') return null
  const { url, hand } = value as { url?: unknown; hand?: unknown }
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
  return { gesture, side, url, ...(points.length > 0 ? { hand: points } : {}) }
}
