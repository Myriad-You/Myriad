import assert from 'node:assert/strict'
import test from 'node:test'
import { FACE_PRESETS, PLAIN_FACE, SCORE_MOVES } from '@myriad/figure/workbench'
import zh from '../../../i18n/merope.zh-CN.json' with { type: 'json' }
import { EXPRESSION_GROUPS, FACE_PRESET_BUTTONS, FACE_VIA_CHOICES, facePresetDriver, facePresetLabel, faceViaLabel, PRESETS, SCORE_MOVE_BUTTONS, scoreMoveNow } from './motionControls'

test('every look the director can pick is a named button in the workbench', () => {
  assert.deepEqual(FACE_PRESET_BUTTONS.map((preset) => preset.id), Object.keys(FACE_PRESETS))
  for (const { id, driver } of FACE_PRESET_BUTTONS) {
    assert.notEqual(facePresetLabel(zh as never, id), id, id)
    assert.ok(Object.keys(driver).length > 0, id)
  }
})

test('a look plays on the drivers at its tiers, as a directive would', () => {
  const content = facePresetDriver('content')
  assert.equal(content.eyeOpenL, 0)
  assert.equal(content.eyeSmile, 0.65)
  assert.equal(content.mouthForm, 0.65)
  assert.equal(facePresetDriver('humming').accentNotes, 1)
  assert.equal(facePresetDriver('blushing').accentBlush, 0.65)
  // Looking down is a positive driver eyeY.
  assert.ok((facePresetDriver('downcast').eyeY ?? 0) > 0)
})

test('every expression is in exactly one group, by whether it moves', () => {
  const ids = EXPRESSION_GROUPS.flatMap((group) => group.buttons.map((button) => `${button.kind}:${button.id}`))
  assert.equal(new Set(ids).size, ids.length)
  assert.deepEqual(
    new Set(ids),
    new Set([...PRESETS.map((preset) => `drawn:${preset.id}`), ...Object.keys(FACE_PRESETS).map((id) => `face:${id}`)]),
  )
  for (const group of EXPRESSION_GROUPS) assert.ok(zh[group.title as keyof typeof zh], group.title)
})

test('the looks passed on the way are the ones the director can pass, each named', () => {
  for (const id of FACE_VIA_CHOICES) {
    if (id !== 'none') assert.ok(id === PLAIN_FACE || id in FACE_PRESETS, id)
    assert.notEqual(faceViaLabel(zh as never, id), id, id)
  }
})

test('every move but the blink is a button, played now as one beat the way a score plays it', () => {
  assert.deepEqual(new Set(SCORE_MOVE_BUTTONS.map(([kind]) => kind)), new Set(Object.keys(SCORE_MOVES).filter((kind) => kind !== 'blink')))
  for (const [, label] of SCORE_MOVE_BUTTONS) assert.ok(zh[label], label)
  const nod = scoreMoveNow('nod', 1234.5)
  assert.equal(nod.beats[0].atMs, 1234.5)
  assert.deepEqual(nod.beats[0].move, { kind: 'nod', amount: 1, count: 2, tempo: 1 })
  assert.equal(scoreMoveNow('shrug', 0).beats[0].move?.count, 1)
})
