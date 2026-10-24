import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { noteRelativeUntil, noteSchedulePresets } from './noteSchedulePresets'

const at = (h: number, m = 0, day = 24) => new Date(2026, 9, day, h, m).getTime()

describe('noteSchedulePresets', () => {
  it('上午：一小时后、明早九点、今晚八点，按先后排', () => {
    const presets = noteSchedulePresets(at(10, 2))
    assert.deepEqual(presets.map((p) => p.key), ['hour', 'tonight', 'tomorrowMorning'])
    assert.equal(presets[0]!.at, at(11, 5))
    assert.equal(presets[1]!.at, at(20))
    assert.equal(presets[2]!.at, at(9, 0, 25))
  })

  it('今晚八点不到一小时就换成明晚', () => {
    const presets = noteSchedulePresets(at(19, 30))
    assert.deepEqual(presets.map((p) => p.key), ['hour', 'tomorrowMorning', 'tomorrowEvening'])
    assert.equal(presets[2]!.at, at(20, 0, 25))
  })
})

describe('noteRelativeUntil', () => {
  it('分钟、小时、天三档', () => {
    assert.equal(noteRelativeUntil(at(10, 20), at(10), 'en'), 'in 20 minutes')
    assert.equal(noteRelativeUntil(at(9, 0, 25), at(10), 'en'), 'in 23 hours')
    assert.equal(noteRelativeUntil(at(10, 0, 28), at(10), 'en'), 'in 4 days')
    assert.equal(noteRelativeUntil(at(9), at(10), 'en'), 'in 1 minute')
  })
})
