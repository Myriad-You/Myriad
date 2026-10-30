import type { MeropeRigManifest, RigBone } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { buildRigSemantics, resolveRigSemantics } from './semantics'

const customBones: RigBone[] = [
  { id: 'hips-x', parent: null, pivot: { x: 0.5, y: 0.8 } },
  { id: 'spine-x', parent: 'hips-x', pivot: { x: 0.5, y: 0.5 } },
  { id: 'skull-x', parent: 'spine-x', pivot: { x: 0.5, y: 0.25 } },
  { id: 'painted-hands-x', parent: 'spine-x', pivot: { x: 0.5, y: 0.68 } },
  { id: 'fabric-17', parent: 'spine-x', pivot: { x: 0.55, y: 0.7 } },
]

test('builds explicit canonical FaceRig semantics once during import', () => {
  const semantics = buildRigSemantics([
    { id: 'root', parent: null, pivot: { x: 0.5, y: 0.8 } },
    { id: 'body', parent: 'root', pivot: { x: 0.5, y: 0.5 } },
    { id: 'head', parent: 'body', pivot: { x: 0.5, y: 0.25 } },
    {
      id: 'a25d-handwear',
      parent: 'body',
      pivot: { x: 0.5, y: 0.68 },
    },
  ])
  assert.equal(semantics.bones.torso, 'body')
  assert.equal(semantics.bones.handwear, 'a25d-handwear')
  assert.deepEqual(semantics.chains.torso, ['root', 'body', 'head'])
})

test('runtime consumes custom torso and whole-handwear mappings', () => {
  const manifest = {
    bones: customBones,
    semantics: {
      bones: {
        root: 'hips-x',
        torso: 'spine-x',
        head: 'skull-x',
        handwear: 'painted-hands-x',
      },
      chains: { torso: ['hips-x', 'spine-x', 'skull-x'] },
      secondaryBoneIds: ['fabric-17'],
    },
  } as MeropeRigManifest
  const semantics = resolveRigSemantics(manifest)
  assert.equal(semantics.bones.torso, 'spine-x')
  assert.equal(semantics.bones.handwear, 'painted-hands-x')
  assert.deepEqual(semantics.chains.torso, ['hips-x', 'spine-x', 'skull-x'])
  assert.deepEqual(semantics.secondaryBoneIds, ['fabric-17'])
})
