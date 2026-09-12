import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  applyRangeClick,
  buildMonthGrid,
  inclusiveDaySpan,
  rangeDayState,
  weekdayMon0,
} from './dateCalendarLogic'

describe('dateCalendarLogic', () => {
  it('weekdayMon0: 2026-08-03 is Monday', () => {
    assert.equal(weekdayMon0(2026, 8, 3), 0)
  })

  it('buildMonthGrid has 42 cells and correct first day offset', () => {
    const grid = buildMonthGrid(2026, 8)
    assert.equal(grid.length, 42)
    assert.equal(grid[0]!.kind, 'empty')
    assert.equal(grid[5]!.kind, 'day')
    if (grid[5]!.kind === 'day') {
      assert.equal(grid[5]!.day, 1)
      assert.equal(grid[5]!.iso, '2026-08-01')
    }
  })

  it('applyRangeClick picks from then to, swaps if reversed', () => {
    const a = applyRangeClick('2026-08-01', {
      from: '',
      to: '',
      picking: 'from',
    })
    assert.deepEqual(a, {
      from: '2026-08-01',
      to: '',
      picking: 'to',
      complete: false,
    })
    const b = applyRangeClick('2026-08-10', {
      from: a.from,
      to: a.to,
      picking: a.picking,
    })
    assert.equal(b.complete, true)
    assert.equal(b.from, '2026-08-01')
    assert.equal(b.to, '2026-08-10')
    const c = applyRangeClick('2026-07-20', {
      from: '2026-08-01',
      to: '',
      picking: 'to',
    })
    assert.equal(c.from, '2026-07-20')
    assert.equal(c.to, '2026-08-01')
  })

  it('rangeDayState marks endpoints and interior', () => {
    assert.equal(
      rangeDayState('2026-08-05', {
        from: '2026-08-01',
        to: '2026-08-10',
      }),
      'in-range',
    )
    assert.equal(
      rangeDayState('2026-08-01', {
        from: '2026-08-01',
        to: '2026-08-10',
      }),
      'start',
    )
    assert.equal(
      rangeDayState('2026-08-10', {
        from: '2026-08-01',
        to: '2026-08-10',
      }),
      'end',
    )
  })

  it('inclusiveDaySpan', () => {
    assert.equal(inclusiveDaySpan('2026-08-01', '2026-08-01'), 1)
    assert.equal(inclusiveDaySpan('2026-08-01', '2026-08-10'), 10)
  })
})
