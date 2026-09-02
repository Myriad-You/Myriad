import type {
  MeropeRigManifest,
  RigBone,
  RigSemanticBoneRole,
  RigSemanticChainRole,
  RigSemantics,
} from './types'
import { RIG_SECONDARY_PART_PATTERNS } from './contract'

const CANONICAL_BONE_IDS: Readonly<Record<RigSemanticBoneRole, string>> = {
  root: 'root',
  torso: 'body',
  head: 'head',
  face: 'face',
  'left-eye': 'left-eye',
  'right-eye': 'right-eye',
  mouth: 'mouth',
  handwear: 'a25d-handwear',
}

const CHAIN_ROLES: Readonly<
  Record<RigSemanticChainRole, readonly RigSemanticBoneRole[]>
> = {
  torso: ['root', 'torso', 'head'],
}

const resolvedCache = new WeakMap<MeropeRigManifest, RigSemantics>()

/** Produces the semantic IR at import/migration time, while names are known. */
export function buildRigSemantics(
  bones: readonly RigBone[],
  explicitSecondaryBoneIds: readonly string[] = [],
): RigSemantics {
  const ids = new Set(bones.map((bone) => bone.id))
  const semanticBones: RigSemantics['bones'] = {}
  for (const [role, canonicalId] of Object.entries(CANONICAL_BONE_IDS) as Array<
    [RigSemanticBoneRole, string]
  >) {
    if (ids.has(canonicalId)) semanticBones[role] = canonicalId
  }
  const chains: RigSemantics['chains'] = {}
  for (const [role, boneRoles] of Object.entries(CHAIN_ROLES) as Array<
    [RigSemanticChainRole, readonly RigSemanticBoneRole[]]
  >) {
    const chain = boneRoles.flatMap((boneRole) =>
      semanticBones[boneRole] ? [semanticBones[boneRole]] : [],
    )
    if (chain.length === boneRoles.length && connected(chain, bones)) {
      chains[role] = chain
    }
  }
  const inferredSecondary = bones
    .map((bone) => bone.id)
    .filter((id) =>
      RIG_SECONDARY_PART_PATTERNS.some((pattern) =>
        id.toLowerCase().includes(pattern),
      ),
    )
  return {
    bones: semanticBones,
    chains,
    secondaryBoneIds: [
      ...new Set(
        [...explicitSecondaryBoneIds, ...inferredSecondary].filter((id) =>
          ids.has(id),
        ),
      ),
    ],
  }
}

/** Resolves legacy manifests through canonical fallback without mutating them. */
export function resolveRigSemantics(manifest: MeropeRigManifest): RigSemantics {
  const cached = resolvedCache.get(manifest)
  if (cached) return cached
  const inferred = buildRigSemantics(
    manifest.bones,
    manifest.outfitProfile?.secondaryPartIds,
  )
  const semantics = manifest.semantics
    ? {
        bones: {
          ...inferred.bones,
          ...supportedBoneMappings(manifest.semantics.bones),
        },
        chains: {
          ...inferred.chains,
          ...supportedChainMappings(manifest.semantics.chains),
        },
        secondaryBoneIds: [
          ...new Set([
            ...inferred.secondaryBoneIds,
            ...manifest.semantics.secondaryBoneIds,
          ]),
        ],
      }
    : inferred
  resolvedCache.set(manifest, semantics)
  return semantics
}

function supportedBoneMappings(
  mappings: Readonly<Record<string, string | undefined>>,
): RigSemantics['bones'] {
  const supported = new Set<string>(Object.keys(CANONICAL_BONE_IDS))
  return Object.fromEntries(
    Object.entries(mappings).filter(
      (entry): entry is [RigSemanticBoneRole, string] =>
        supported.has(entry[0]) && typeof entry[1] === 'string',
    ),
  )
}

function supportedChainMappings(
  mappings: Readonly<Record<string, string[] | undefined>>,
): RigSemantics['chains'] {
  const supported = new Set<string>(Object.keys(CHAIN_ROLES))
  return Object.fromEntries(
    Object.entries(mappings).filter(
      (entry): entry is [RigSemanticChainRole, string[]] =>
        supported.has(entry[0]) && Array.isArray(entry[1]),
    ),
  )
}

function connected(
  chain: readonly string[],
  bones: readonly RigBone[],
): boolean {
  const parents = new Map(bones.map((bone) => [bone.id, bone.parent]))
  return chain.slice(1).every((boneId, index) => {
    const previous = chain[index]
    return parents.get(boneId) === previous || parents.get(previous) === boneId
  })
}
