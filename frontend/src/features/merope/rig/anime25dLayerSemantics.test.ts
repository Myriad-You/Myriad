import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DBaseRole,
  anime25DLayerGroup,
  isAnime25DRigidAttachment,
  normalizeAnime25DLayerName,
  resolveAnime25DLayerSemantics,
} from './anime25dLayerSemantics'

test('recognizes every in-scope See-through v3 category without inventing bones', () => {
  for (const name of [
    'front hair',
    'back hair',
    'headwear',
    'face',
    'irides',
    'eyebrow',
    'eyewhite',
    'eyelash',
    'eyewear',
    'ears',
    'earwear',
    'nose',
    'mouth',
    'neck',
    'neckwear',
    'topwear',
    'handwear',
    'bottomwear',
    'tail',
    'wings',
    'objects',
  ]) {
    assert.ok(anime25DBaseRole(normalizeAnime25DLayerName(name)), name)
  }
  assert.equal(anime25DBaseRole('legwear'), null)
  assert.equal(anime25DBaseRole('footwear'), null)
})

test('aliases retain combined depth, index and side suffixes', () => {
  for (const [input, name, role] of [
    [' hairf_2 のコピー 3 ', 'front-hair-2', 'front-hair'],
    ['Necklace_1_L', 'neckwear-1-l', 'neckwear'],
    ['eyewear_left_2', 'eyewear-left-2', 'eyewear'],
    ['earrings_2_bottom_R', 'earwear-2-bottom-r', 'earwear'],
  ]) {
    assert.equal(normalizeAnime25DLayerName(input), name)
    assert.equal(anime25DBaseRole(name), role)
  }
  assert.equal(
    anime25DBaseRole(normalizeAnime25DLayerName('necklace-shadow')),
    null,
  )
  assert.equal(anime25DBaseRole('my-unknown-ornament'), null)
})

test('named accessories override a misleading centroid group without mutating stored art', () => {
  const original = Object.freeze({
    name: 'eyewear',
    role: 'unknown',
    group: 'body' as const,
    depth: 1,
    atlas: { x: 0.3 },
  })
  const resolved = resolveAnime25DLayerSemantics(original)
  assert.equal(resolved.role, 'eyewear')
  assert.equal(resolved.group, 'head')
  assert.equal(resolved.atlas, original.atlas)
  assert.equal(original.role, 'unknown')
  assert.equal(anime25DLayerGroup('neckwear', 'head'), 'body')
  assert.equal(anime25DLayerGroup('objects', 'head'), 'head')
  const unknown = { name: 'mystery', role: 'unknown', group: 'body' as const }
  assert.equal(resolveAnime25DLayerSemantics(unknown), unknown)
})

test('regional accessory labels never imply cloth deformation or expression physics', () => {
  for (const role of [
    'neckwear',
    'eyewear',
    'headwear',
    'earwear',
    'wings',
    'tail',
    'objects',
    'unknown',
  ]) {
    assert.equal(isAnime25DRigidAttachment({ role }), true)
  }
  for (const role of ['topwear', 'neck', 'face', 'front-hair'])
    assert.equal(isAnime25DRigidAttachment({ role }), false)
  assert.equal(
    isAnime25DRigidAttachment({ role: 'unknown', fade: 'eyeCry' }),
    false,
  )
  assert.equal(
    isAnime25DRigidAttachment({ role: 'unknown', phys: 'hair' }),
    false,
  )
})
