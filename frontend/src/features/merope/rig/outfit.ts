import type { RigOutfitProfile, RigOutfitTopology } from './types'
import { RIG_OUTFIT_SAFETY, RIG_SECONDARY_PART_PATTERNS } from './contract'
import { RIG_OUTFIT_TOPOLOGIES } from './types'

interface OutfitSafetyRule {
  torsoTwistScale: number
  secondaryMotionScale: number
}

const OUTFIT_SAFETY_RULES = RIG_OUTFIT_SAFETY satisfies Readonly<
  Record<RigOutfitTopology, OutfitSafetyRule>
>

export function createOutfitProfile(
  requestedTopologies: readonly RigOutfitTopology[],
  secondaryPartIds: readonly string[] = [],
): RigOutfitProfile {
  const topologies = uniqueInCanonicalOrder(requestedTopologies)
  if (topologies.length === 0) topologies.push('fitted')
  const rules = topologies.map((topology) => OUTFIT_SAFETY_RULES[topology])
  return {
    topologies,
    secondaryPartIds: [...new Set(secondaryPartIds)],
    torsoTwistScale: Math.min(...rules.map((rule) => rule.torsoTwistScale)),
    secondaryMotionScale: Math.min(
      ...rules.map((rule) => rule.secondaryMotionScale),
    ),
  }
}

export function inferOutfitProfileFromPartIds(
  partIds: readonly string[],
): RigOutfitProfile {
  const ids = partIds.map((id) => id.toLowerCase())
  const has = (...patterns: string[]) =>
    ids.some((id) => patterns.some((pattern) => id.includes(pattern)))
  const topologies: RigOutfitTopology[] = []
  if (has('long-skirt', 'maxi-skirt', 'dress-hem')) {
    topologies.push('long-skirt')
  } else if (has('skirt', 'dress')) {
    topologies.push('short-skirt')
  }
  if (has('coat-tail', 'long-coat', 'trench')) topologies.push('long-coat')
  if (has('wide-sleeve', 'kimono-sleeve', 'bell-sleeve')) {
    topologies.push('wide-sleeve')
  }
  if (has('cape', 'cloak')) topologies.push('cape')
  if (has('armor', 'pauldron', 'plate')) topologies.push('armor')
  return createOutfitProfile(
    topologies,
    partIds.filter(isSecondaryMotionPartId),
  )
}

function isSecondaryMotionPartId(partId: string): boolean {
  const normalized = partId.toLowerCase()
  return RIG_SECONDARY_PART_PATTERNS.some((pattern) =>
    normalized.includes(pattern),
  )
}

function uniqueInCanonicalOrder(
  requested: readonly RigOutfitTopology[],
): RigOutfitTopology[] {
  const unique = new Set(requested)
  return RIG_OUTFIT_TOPOLOGIES.filter((topology) => unique.has(topology))
}
