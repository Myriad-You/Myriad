import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  clampSnapshotLimit,
  SNAPSHOT_LIMIT_DEFAULT,
  SNAPSHOT_LIMIT_MAX,
  SNAPSHOT_LIMIT_MIN,
  SNAPSHOT_LIMIT_PRESETS,
} from './updaterApi.ts'

describe('snapshot limit range 1–20', () => {
  it('presets stay inside 1–20 and include max', () => {
    for (const n of SNAPSHOT_LIMIT_PRESETS) {
      assert.ok(
        n >= SNAPSHOT_LIMIT_MIN && n <= SNAPSHOT_LIMIT_MAX,
        `preset ${n}`,
      )
    }
    assert.ok(SNAPSHOT_LIMIT_PRESETS.includes(SNAPSHOT_LIMIT_MAX as 20))
    assert.ok(SNAPSHOT_LIMIT_PRESETS.includes(SNAPSHOT_LIMIT_MIN as 1))
  })

  it('clampSnapshotLimit bounds NaN / out-of-range', () => {
    assert.equal(clampSnapshotLimit(Number.NaN), SNAPSHOT_LIMIT_DEFAULT)
    assert.equal(clampSnapshotLimit(0), SNAPSHOT_LIMIT_MIN)
    assert.equal(clampSnapshotLimit(-5), SNAPSHOT_LIMIT_MIN)
    assert.equal(clampSnapshotLimit(21), SNAPSHOT_LIMIT_MAX)
    assert.equal(clampSnapshotLimit(7.4), 7)
    assert.equal(clampSnapshotLimit(20), 20)
  })
})
