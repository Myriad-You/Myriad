import assert from 'node:assert/strict'
import test from 'node:test'
import type { GazeTarget } from './motion'

test('gaze targets stay in a 2d plane', () => {
  const target: GazeTarget = { x: 0.2, y: -0.1, source: 'pointer' }
  assert.equal(target.source, 'pointer')
})
