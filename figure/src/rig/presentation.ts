import type { MeropeRigManifest } from './types'
import { RIG_PRESENTATION_SLOTS } from './contract'

interface PresentationSlotDefinition {
  fallback: string
  variants: readonly string[]
}

function presentationSlotDefinition(
  slot: string,
): PresentationSlotDefinition | undefined {
  return (RIG_PRESENTATION_SLOTS as Record<string, PresentationSlotDefinition>)[
    slot
  ]
}

export function presentationAssetCoverage(
  manifest: MeropeRigManifest,
): Array<{ slot: string; missingFallback: string | null; unknown: string[] }> {
  const variantsBySlot = new Map<string, Set<string>>()
  for (const part of manifest.parts ?? []) {
    if (!part.slot || !part.variant) continue
    const variants = variantsBySlot.get(part.slot) ?? new Set<string>()
    variants.add(part.variant)
    variantsBySlot.set(part.slot, variants)
  }
  return Iterator.from(variantsBySlot)
    .map(([slot, variants]) => {
      const definition = presentationSlotDefinition(slot)
      return {
        slot,
        missingFallback:
          definition && !variants.has(definition.fallback)
            ? definition.fallback
            : null,
        unknown: definition
          ? Iterator.from(
              variants.difference(new Set(definition.variants)),
            ).toArray()
          : [],
      }
    })
    .toArray()
}
