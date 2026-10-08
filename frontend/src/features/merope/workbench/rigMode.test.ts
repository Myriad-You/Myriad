import assert from 'node:assert/strict'
import test from 'node:test'
import { activeRigMode } from './rigMode'

function manifest(master: string, extra: Record<string, unknown> = {}) {
  return {
    sourceMasterAssetId: master,
    anime25dPlayback: { layers: [], ...extra },
  } as never
}

test('the mode the rig was made in, for its own portrait only', () => {
  assert.equal(activeRigMode(null, '/a.png'), null)
  assert.equal(activeRigMode({ sourceMasterAssetId: '/a.png' }, '/a.png'), null, 'no playback')
  assert.equal(activeRigMode(manifest('/a.png'), '/b.png'), null, 'a rig of another portrait')
  assert.equal(activeRigMode(manifest('/a.png'), '/a.png'), 'plain')
  assert.equal(activeRigMode(manifest('/a.png', { turnKeyforms: {} }), '/a.png'), 'enhanced', 'keyed before the record')
  assert.equal(activeRigMode(manifest('/a.png', { enhancement: { turn: false, expressions: true } }), '/a.png'), 'enhanced')
  assert.equal(activeRigMode(manifest('/a.png', { enhancement: { turn: false, expressions: false } }), '/a.png'), 'plain')
})
