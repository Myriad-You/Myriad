import type { MeropeRigManifest } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { deriveGeometryChestProfile } from '../anime25drig/chestPhysics'
import { ANIME25D_PLAYBACK_VERSION } from '../anime25drig/credit'
import { analyzeAnime25DMouthProfile } from '../anime25drig/mouthProfile'
import { deriveAnime25DShellProfile } from '../anime25drig/shellProfile'
import { RIG_IR_VERSION } from './contract'
import { isLiveMeropeManifest, isRigManifest } from './types'

const manifest: MeropeRigManifest = {
  schemaVersion: 1,
  quality: 'layered-2d',
  canvas: { width: 1, height: 1 },
  textures: [{ id: 'atlas', url: '/atlas.png', width: 512, height: 512 }],
  bones: [
    { id: 'handwear', parent: 'root', pivot: { x: 0.8, y: 0.5 } },
    { id: 'root', parent: null, pivot: { x: 0.5, y: 0.8 } },
  ],
  parts: [
    {
      id: 'handwear',
      textureId: 'atlas',
      zIndex: 1,
      opacity: 1,
      vertices: [
        {
          position: { x: 0, y: 0 },
          uv: { x: 0, y: 0 },
          joints: [0, 1, 0, 0],
          weights: [0.8, 0.2, 0, 0],
        },
        {
          position: { x: 1, y: 0 },
          uv: { x: 1, y: 0 },
          joints: [0, 1, 0, 0],
          weights: [0.8, 0.2, 0, 0],
        },
        {
          position: { x: 0, y: 1 },
          uv: { x: 0, y: 1 },
          joints: [0, 1, 0, 0],
          weights: [0.8, 0.2, 0, 0],
        },
      ],
      indices: [0, 1, 2],
    },
  ],
}

test('accepts bounded manifests with parent-after-child bones', () => {
  assert.equal(isRigManifest(manifest), true)
})

test('rejects leftover clip-stack fields', () => {
  const leftover = structuredClone(manifest) as Record<string, unknown>
  leftover.clips = []
  assert.equal(isRigManifest(leftover), false)
  delete leftover.clips
  leftover.defaultClip = 'idle'
  assert.equal(isRigManifest(leftover), false)
  delete leftover.defaultClip
  leftover.standardClipLibraryVersion = 2
  assert.equal(isRigManifest(leftover), false)
})

test('only treats a layered Anime2.5D package as a live site face', () => {
  assert.equal(isLiveMeropeManifest(manifest), false)
  const live = structuredClone(manifest)
  const mouth = {
    x0: 0.4,
    y0: 0.4,
    x1: 0.6,
    y1: 0.5,
    cx: 0.5,
    cy: 0.45,
  }
  const playbackSource = {
    kind: 'anime-2.5d-rig',
    version: ANIME25D_PLAYBACK_VERSION,
    engine: 'Anime2.5DRig',
    engineUrl: 'https://github.com/852wa/Anime2.5DRig',
    license: 'MIT',
    copyright: 'Copyright (c) 2026 hakoniwa',
    pixelCanvas: { width: 1152, height: 1536 },
    layers: [
      {
        name: 'face',
        role: 'face',
        depth: 0,
        group: 'head',
        phys: null,
        fade: null,
        side: null,
        x: 0,
        y: 0,
        w: 1,
        h: 1,
        atlas: { x: 0, y: 0, w: 1, h: 1 },
        strands: [],
      },
    ],
    anchors: {
      face: { x0: 0, y0: 0, x1: 1, y1: 1, cx: 0.5, cy: 0.3 },
      neckPivot: { x: 0.5, y: 0.45 },
      neckTop: 0.4,
      neckBottom: 0.5,
      bodyPivot: { x: 0.5, y: 0.7 },
      mouth,
      faceScale: 1,
    },
    mouthProfile: analyzeAnime25DMouthProfile(
      [],
      { x: 0, y: 0, width: 1152, height: 1536 },
      mouth,
    ),
  }
  const shellProfile = deriveAnime25DShellProfile(playbackSource)
  shellProfile.head.radiusX = 1
  shellProfile.head.radiusY = 1
  shellProfile.head.radiusZ = 1
  shellProfile.hair.radiusX = 1
  shellProfile.hair.radiusY = 1
  shellProfile.hair.radiusZ = 1
  live.anime25dPlayback = {
    ...playbackSource,
    chestProfile: deriveGeometryChestProfile(playbackSource),
    shellProfile,
  }
  assert.equal(isLiveMeropeManifest(live), true)
  const missingChestProfile = structuredClone(live) as unknown as {
    anime25dPlayback: Record<string, unknown>
  }
  delete missingChestProfile.anime25dPlayback.chestProfile
  assert.equal(isLiveMeropeManifest(missingChestProfile), false)
  const missingShellProfile = structuredClone(live) as unknown as {
    anime25dPlayback: Record<string, unknown>
  }
  delete missingShellProfile.anime25dPlayback.shellProfile
  assert.equal(isLiveMeropeManifest(missingShellProfile), false)
  live.anime25dPlayback.version = 6 as typeof ANIME25D_PLAYBACK_VERSION
  assert.equal(isLiveMeropeManifest(live), false)
  live.anime25dPlayback.version = ANIME25D_PLAYBACK_VERSION
  live.anime25dPlayback.shellProfile.hair.frontGap = 0.7
  assert.equal(isLiveMeropeManifest(live), false)
  live.anime25dPlayback.shellProfile.hair.frontGap = 0.18
  assert.equal(isLiveMeropeManifest(live), true)
  live.anime25dPlayback.shellProfile.torso.radiusZ = 0
  assert.equal(isLiveMeropeManifest(live), false)
  live.anime25dPlayback.shellProfile.torso.radiusZ = 1
  assert.equal(isLiveMeropeManifest(live), true)
  live.anime25dPlayback.chestProfile = {
    version: 2,
    enabled: true,
    source: 'ai-vision',
    centerX: 576,
    centerY: 1120,
    radiusX: 210,
    radiusY: 180,
    visibleScale: 0.7,
    motionScale: 1.05,
    frequencyScale: 0.96,
    supportScale: 0.35,
    garmentMotionScale: 0.8,
    confidence: 0.9,
  }
  assert.equal(isLiveMeropeManifest(live), true)
  live.anime25dPlayback.chestProfile.supportScale = 1.1
  assert.equal(isLiveMeropeManifest(live), false)
  live.anime25dPlayback.chestProfile.supportScale = 0.35
  assert.equal(isLiveMeropeManifest(live), true)
  live.anime25dPlayback.chestProfile.enabled = false
  live.anime25dPlayback.chestProfile.source = 'gender-policy'
  live.anime25dPlayback.chestProfile.visibleScale = 0
  live.anime25dPlayback.chestProfile.motionScale = 0
  live.anime25dPlayback.chestProfile.supportScale = 1
  live.anime25dPlayback.chestProfile.garmentMotionScale = 0
  live.anime25dPlayback.chestProfile.confidence = 1
  assert.equal(isLiveMeropeManifest(live), true)
  live.anime25dPlayback.chestProfile.radiusX = 900
  assert.equal(isLiveMeropeManifest(live), false)
})

test('validates portrait generation provenance when present', () => {
  const generated = structuredClone(manifest)
  generated.sourceGenerationFingerprint = 'a'.repeat(64)
  assert.equal(isRigManifest(generated), true)
  generated.sourceGenerationFingerprint = 'not-a-sha256'
  assert.equal(isRigManifest(generated), false)
})

test('requires known presentation variants and a stable slot fallback', () => {
  const variant = structuredClone(manifest)
  variant.rigIrVersion = RIG_IR_VERSION
  variant.semantics = {
    bones: { root: 'root' },
    chains: {},
    secondaryBoneIds: [],
  }
  variant.spatialProfile = { collisionVolumes: [] }
  variant.parts[0].slot = 'mouth'
  variant.parts[0].variant = 'closed'
  assert.equal(isRigManifest(variant), true)
  variant.parts[0].variant = 'open'
  assert.equal(isRigManifest(variant), false)
  variant.parts.push({
    ...structuredClone(variant.parts[0]),
    id: 'mouth-closed',
    variant: 'closed',
  })
  assert.equal(isRigManifest(variant), true)
  variant.parts[0].variant = 'invented'
  assert.equal(isRigManifest(variant), false)
  variant.parts[0].slot = 'invented-slot'
  assert.equal(isRigManifest(variant), false)
})

test('keeps supported IR v2 custom slots readable for non-destructive migration', () => {
  const legacy = structuredClone(manifest)
  legacy.rigIrVersion = 2
  legacy.semantics = {
    bones: { root: 'root' },
    chains: {},
    secondaryBoneIds: [],
  }
  legacy.spatialProfile = { collisionVolumes: [] }
  legacy.parts[0].slot = 'custom-emblem'
  legacy.parts[0].variant = 'lit'
  assert.equal(isRigManifest(legacy), true)
  legacy.rigIrVersion = RIG_IR_VERSION
  assert.equal(isRigManifest(legacy), false)
})

test('validates versioned semantic IR and connected custom chains', () => {
  const semantic = structuredClone(manifest)
  semantic.bones.push(
    { id: 'body', parent: 'root', pivot: { x: 0.5, y: 0.55 } },
    { id: 'head', parent: 'body', pivot: { x: 0.5, y: 0.25 } },
  )
  semantic.rigIrVersion = RIG_IR_VERSION
  semantic.semantics = {
    bones: {
      root: 'root',
      torso: 'body',
      head: 'head',
      handwear: 'handwear',
    },
    chains: { torso: ['root', 'body', 'head'] },
    secondaryBoneIds: [],
  }
  semantic.spatialProfile = {
    collisionVolumes: [
      {
        id: 'torso',
        boneId: 'root',
        offset: { x: 0, y: 0 },
        radius: { x: 0.2, y: 0.3 },
        padding: 0.01,
      },
    ],
  }
  assert.equal(isRigManifest(semantic), true)
  semantic.semantics.chains.torso = ['head', 'body', 'root']
  assert.equal(isRigManifest(semantic), true)
  semantic.semantics.bones.root = 'missing'
  assert.equal(isRigManifest(semantic), false)
  semantic.semantics.bones.root = 'root'
  semantic.rigIrVersion = RIG_IR_VERSION + 1
  assert.equal(isRigManifest(semantic), false)
})

test('rejects invalid character collision volumes', () => {
  const spatial = structuredClone(manifest)
  spatial.rigIrVersion = RIG_IR_VERSION
  spatial.semantics = {
    bones: { root: 'root' },
    chains: {},
    secondaryBoneIds: [],
  }
  spatial.spatialProfile = {
    collisionVolumes: [
      {
        id: 'torso',
        boneId: 'root',
        offset: { x: 0, y: 0 },
        radius: { x: 0, y: 0.3 },
        padding: 0.01,
      },
    ],
  }
  assert.equal(isRigManifest(spatial), false)
})

test('rejects cyclic bone hierarchies', () => {
  const invalid = structuredClone(manifest)
  invalid.bones[1].parent = 'hand'
  assert.equal(isRigManifest(invalid), false)
})

test('rejects non-normalized skin weights', () => {
  const invalid = structuredClone(manifest)
  invalid.parts[0].vertices[0].weights = [0.2, 0.2, 0.2, 0]
  assert.equal(isRigManifest(invalid), false)
})

test('accepts a bounded optional procedural motion profile', () => {
  const profiled = structuredClone(manifest)
  profiled.motionProfile = {
    seed: 42,
    breath: { minFrequencyHz: 0.16, maxFrequencyHz: 0.28, amplitude: 0.004 },
    blink: {
      minIntervalSeconds: 2.5,
      maxIntervalSeconds: 7,
      durationSeconds: 0.24,
      doubleChance: 0.15,
    },
    secondary: {
      enabled: true,
      frequencyHz: 2.1,
      dampingRatio: 0.5,
      response: 0.6,
    },
  }
  assert.equal(isRigManifest(profiled), true)
})

test('rejects unstable or unbounded motion profiles', () => {
  const invalid = structuredClone(manifest)
  invalid.motionProfile = {
    seed: 42,
    breath: { minFrequencyHz: 0.16, maxFrequencyHz: 0.28, amplitude: 0.4 },
    blink: {
      minIntervalSeconds: 2.5,
      maxIntervalSeconds: 7,
      durationSeconds: 0.24,
      doubleChance: 0.15,
    },
    secondary: {
      enabled: true,
      frequencyHz: 2.1,
      dampingRatio: -0.5,
      response: 0.6,
    },
  }
  assert.equal(isRigManifest(invalid), false)
})

test('accepts bounded outfit safety and character-local semantic anchors', () => {
  const outfitted = structuredClone(manifest)
  outfitted.outfitProfile = {
    topologies: ['wide-sleeve', 'long-skirt'],
    secondaryPartIds: ['handwear'],
    torsoTwistScale: 0.9,
    secondaryMotionScale: 0.78,
  }
  outfitted.semanticAnchors = {
    forehead: { boneId: 'root', offset: { x: 0, y: -0.2 } },
  }
  assert.equal(isRigManifest(outfitted), true)
  outfitted.outfitProfile.topologies = ['armor', 'armor']
  assert.equal(isRigManifest(outfitted), false)
  outfitted.outfitProfile.topologies = ['armor']
  outfitted.outfitProfile.secondaryPartIds = ['handwear', 'handwear']
  assert.equal(isRigManifest(outfitted), false)
  outfitted.outfitProfile.secondaryPartIds = ['handwear']
  outfitted.outfitProfile.torsoTwistScale = 0.1
  assert.equal(isRigManifest(outfitted), false)
  outfitted.outfitProfile.torsoTwistScale = 0.9
  outfitted.outfitProfile.secondaryMotionScale = 1.2
  assert.equal(isRigManifest(outfitted), false)
})
