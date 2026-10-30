import assert from 'node:assert/strict'
import test from 'node:test'
import {
  hasAnime25DCapability,
  missingAnime25DRequiredCapabilities,
} from './anime25dCapabilities'

test('evaluates both importer roles and compiled a25d layer ids', () => {
  assert.equal(
    hasAnime25DCapability(
      [
        { id: 'lovestruck-heart-left', role: 'lovestruck-heart', side: 'left' },
        { id: 'a25d-lovestruck-heart-right' },
      ],
      'lovestruck-heart-pupils',
    ),
    true,
  )
})

test('fails closed when the shared contract introduces an unknown capability', () => {
  assert.ok(missingAnime25DRequiredCapabilities([]).length > 0)
  assert.equal(hasAnime25DCapability([], 'future-capability'), false)
})

test('hair drawn all in front leaves no back hair to separate', () => {
  assert.equal(hasAnime25DCapability([{ id: 'front hair', role: 'front-hair' }], 'separate-back-hair'), true)
  assert.equal(hasAnime25DCapability([{ id: 'a25d-back-hair' }], 'separate-back-hair'), true)
  assert.equal(hasAnime25DCapability([{ id: 'face', role: 'face' }], 'separate-back-hair'), false)
})
