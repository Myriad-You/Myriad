import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  pickLibraryTourCardId,
  pinLibraryTourCard,
} from './libraryCanvasVisible'

describe('pickLibraryTourCardId', () => {
  it('returns the first list card when layouts are absent', () => {
    assert.equal(
      pickLibraryTourCardId([{ id: 'a' }, { id: 'b' }]),
      'a',
    )
    assert.equal(pickLibraryTourCardId([]), null)
  })

  it('pins the canvas card closest to world origin', () => {
    const layouts = new Map([
      ['far', { left: 400, top: 200, width: 180, height: 180 }],
      ['near', { left: -40, top: -20, width: 180, height: 180 }],
      ['edge', { left: 80, top: 90, width: 180, height: 180 }],
    ])
    assert.equal(
      pickLibraryTourCardId([{ id: 'far' }, { id: 'near' }, { id: 'edge' }], layouts),
      'near',
    )
  })
})

describe('pinLibraryTourCard', () => {
  it('appends a dropped tour card so virtualization cannot unmount it', () => {
    const visible = [{ id: 'a' }, { id: 'b' }]
    const laidOut = [{ id: 'a' }, { id: 'b' }, { id: 'origin' }]
    assert.deepEqual(pinLibraryTourCard(visible, laidOut, 'origin'), [
      { id: 'a' },
      { id: 'b' },
      { id: 'origin' },
    ])
    assert.equal(pinLibraryTourCard(visible, laidOut, 'a'), visible)
    assert.deepEqual(pinLibraryTourCard(visible, laidOut, 'missing'), visible)
  })
})
