import assert from 'node:assert/strict'
import test from 'node:test'
import { normalizePsdLayerName } from './psdImporter'

test('normalizes See-through and copied FaceRig PSD layer names', () => {
  assert.equal(
    normalizePsdLayerName(' Front Hair_1 のコピー 2 '),
    'front-hair-1',
  )
  assert.equal(normalizePsdLayerName('mouth_open'), 'mouth-open')
  assert.equal(normalizePsdLayerName('eyelash_c'), 'eye-close')
})
