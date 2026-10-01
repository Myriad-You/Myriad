import type { HerPuzzle } from './herLife'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { askToPlay, hasLife } from './herLife'

const puzzle: HerPuzzle = {
  surface: '暴雪夜，山下的人看到半山腰木屋的灯光有规律地忽明忽暗',
  played: 1,
  solved: 1,
  yours: false,
}

describe('her life in the panel', () => {
  it('asks for her puzzle by its surface, so she brings out that one', () => {
    const asked = askToPlay(puzzle, '来玩你出的这道汤：{surface}')
    // The server knows a puzzle by the start of its surface.
    assert.ok(asked.includes([...puzzle.surface].slice(0, 12).join('')))
    assert.ok(asked.startsWith('来玩你出的这道汤'))
  })

  it('shows nothing until she has some life of her own', () => {
    assert.equal(hasLife(null), false)
    assert.equal(hasLife({ lately: [], wants: [], puzzles: [] }), false)
    assert.equal(hasLife({ lately: [], wants: [], puzzles: [puzzle] }), true)
  })
})
