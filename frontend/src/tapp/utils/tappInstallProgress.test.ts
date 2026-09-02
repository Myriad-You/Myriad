import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  clampInstallPercent,
  isLargeTappInstall,
  LARGE_TAPP_INSTALL_BYTES,
} from './tappInstallProgress.ts'

describe('tappInstallProgress', () => {
  it('flags packages ≥1 MiB', () => {
    assert.equal(isLargeTappInstall(LARGE_TAPP_INSTALL_BYTES - 1), false)
    assert.equal(isLargeTappInstall(LARGE_TAPP_INSTALL_BYTES), true)
    assert.equal(isLargeTappInstall(4_000_000), true)
    assert.equal(isLargeTappInstall(undefined), false)
  })
  it('clamps percent', () => {
    assert.equal(clampInstallPercent(-5), 0)
    assert.equal(clampInstallPercent(150), 100)
    assert.equal(clampInstallPercent(42.4), 42)
  })
})
