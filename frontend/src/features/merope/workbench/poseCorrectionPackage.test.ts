import type { Anime25DPlayback, Anime25DPlaybackV7 } from '@myriad/figure/anime25drig'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { isAnime25DPlayback, readAnime25DPlayback } from '@myriad/figure/anime25drig'
import { storedPoseCorrections } from './poseCorrectionPackage'

// The packages every validator of the package is held to (myriad-merope's
// playback_is_valid, which the save endpoints run, and the player's
// isAnime25DPlayback), one of each version.
const conformance = JSON.parse(readFileSync(
  new URL('../../../../../core/crates/conformance/v1/figure/package.json', import.meta.url),
  'utf8',
)) as { packages: { v7: Anime25DPlaybackV7; v8: Anime25DPlayback } }

function withCorrections<T extends { shellProfile: object }>(playback: T, corrections: unknown): T {
  return { ...playback, shellProfile: { ...playback.shellProfile, poseCorrections: corrections } }
}

test('corrections edited on a version 7 package are saved in version 7\'s axes, as it stored them', () => {
  const stored = conformance.packages.v7
  const edited = readAnime25DPlayback(stored).shellProfile.poseCorrections!
  assert.deepEqual(edited.map(correction => Object.keys(correction.at)), [['headYaw', 'headPitch'], ['headPitch', 'lidCloseL', 'jaw']])

  const saved = storedPoseCorrections(stored, edited)
  assert.deepEqual(saved, stored.shellProfile.poseCorrections)
  assert.ok(isAnime25DPlayback(withCorrections(stored, saved)))
  // What the editor works in is not what a version 7 package takes.
  assert.ok(!isAnime25DPlayback(withCorrections(stored, edited)))
})

test('corrections edited on a version 8 package are saved in its own axes', () => {
  const stored = conformance.packages.v8
  const edited = readAnime25DPlayback(stored).shellProfile.poseCorrections!
  const saved = storedPoseCorrections(stored, edited)
  assert.deepEqual(saved, stored.shellProfile.poseCorrections)
  assert.ok(isAnime25DPlayback(withCorrections(stored, saved)))
})

test('the saved corrections are a copy the editor can go on changing', () => {
  const stored = conformance.packages.v7
  const edited = readAnime25DPlayback(stored).shellProfile.poseCorrections!
  const saved = storedPoseCorrections(stored, edited)
  edited[0]!.patches[0]!.dx = 0.2
  assert.notEqual(saved[0]!.patches[0]!.dx, 0.2)
})
