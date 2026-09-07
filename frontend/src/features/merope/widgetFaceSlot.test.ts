import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'
import {
  claimMeropeWidgetFaceSlot,
  meropeWidgetFaceHolder,
  resetMeropeWidgetFaceSlotForTests,
} from './widgetFaceSlot'

describe('merope widget face slot', { concurrency: false }, () => {
  test('the first live widget keeps the face; the next one waits', () => {
    resetMeropeWidgetFaceSlotForTests()
    const first = claimMeropeWidgetFaceSlot('a')
    const second = claimMeropeWidgetFaceSlot('b')
    assert.equal(meropeWidgetFaceHolder(), 'a')
    first()
    assert.equal(meropeWidgetFaceHolder(), 'b')
    second()
    assert.equal(meropeWidgetFaceHolder(), null)
  })

  test('releasing a waiter does not steal the live face', () => {
    resetMeropeWidgetFaceSlotForTests()
    const first = claimMeropeWidgetFaceSlot('a')
    const second = claimMeropeWidgetFaceSlot('b')
    const third = claimMeropeWidgetFaceSlot('c')
    second()
    assert.equal(meropeWidgetFaceHolder(), 'a')
    first()
    assert.equal(meropeWidgetFaceHolder(), 'c')
    third()
  })

  test('repeat claims for the same widget stay a single queue entry', () => {
    resetMeropeWidgetFaceSlotForTests()
    const first = claimMeropeWidgetFaceSlot('a')
    const again = claimMeropeWidgetFaceSlot('a')
    const other = claimMeropeWidgetFaceSlot('b')
    assert.equal(meropeWidgetFaceHolder(), 'a')
    first()
    assert.equal(meropeWidgetFaceHolder(), 'b')
    again()
    assert.equal(meropeWidgetFaceHolder(), 'b')
    other()
  })
})

test('the live widget is the only one that mounts the rig player', () => {
  const widget = readFileSync(
    new URL('../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  const slot = readFileSync(new URL('./widgetFaceSlot.ts', import.meta.url), 'utf8')
  assert.match(widget, /useMeropeWidgetFaceSlot\(config\.id, !isPreview\)/)
  assert.match(widget, /if \(!holdsFace\) return/)
  assert.match(widget, /t\.merope\.widgetFaceSlotTaken/)
  assert.match(widget, /function MeropeWidgetDuplicate/)
  assert.doesNotMatch(widget, /showTaken/)
  assert.match(widget, /<LiveMeropeWidget/)
  assert.match(widget, /playbackId=\{`widget:\$\{config\.id\}`\}/)
  assert.match(widget, /<RigCharacter/)
  assert.match(widget, /loadPublicPersonaName\(\)\.then/)
  assert.match(slot, /return claimMeropeWidgetFaceSlot\(id\)/)
})

test('panel persona fetch failure does not hide the live face', () => {
  const panel = readFileSync(
    new URL('../../components/agent-panel/AgentPanelFace.tsx', import.meta.url),
    'utf8',
  )
  assert.match(panel, /loadPublicPersonaName\(\)\.then/)
  assert.doesNotMatch(
    panel,
    /\.catch\(\(\) => \{\s*if \(active\) setFailed\(true\)/,
  )
})
