import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  CAPSULE_GAP,
  CAPSULE_WIDTH,
  companionLayout,
  LEADING_STEPS,
  readingPrefsFromValue,
  readingProgress,
  SIZE_STEPS,
  stepFrom,
} from './companionLayout.ts'

const column = (left: number, width: number) => ({ left, right: left + width, top: 0, bottom: 3000 })
const rail = { left: 24, right: 89, top: 300, bottom: 588 }

test('宽窗口里两个胶囊贴着正文两边', () => {
  assert.deepEqual(companionLayout({ width: 1440, height: 900 }, column(400, 640), rail), {
    mode: 'sides',
    left: 400 - CAPSULE_GAP - CAPSULE_WIDTH,
    right: 1040 + CAPSULE_GAP,
  })
})

test('左边会碰上导航岛、右边出窗口、或者太窄时换成底部一条', () => {
  assert.equal(companionLayout({ width: 900, height: 800 }, column(104, 640), rail).mode, 'bar')
  assert.equal(companionLayout({ width: 1100, height: 800 }, column(400, 640), null).mode, 'bar')
  assert.equal(companionLayout({ width: 620, height: 800 }, column(100, 400), null).mode, 'bar')
})

test('手机上导航岛横在底部，底部那条叠在它上面', () => {
  const bar = { left: 49, right: 341, top: 724, bottom: 796 }
  assert.deepEqual(companionLayout({ width: 390, height: 812 }, column(0, 390), bar), {
    mode: 'bar',
    bottom: 812 - 724 + 12,
  })
  assert.deepEqual(companionLayout({ width: 390, height: 812 }, column(0, 390), null), {
    mode: 'bar',
    bottom: 18,
  })
})

test('读到哪了', () => {
  assert.equal(readingProgress({ left: 0, right: 0, top: 100, bottom: 2100 }, 1000), 0)
  assert.equal(readingProgress({ left: 0, right: 0, top: -500, bottom: 1500 }, 1000), 0.5)
  assert.equal(readingProgress({ left: 0, right: 0, top: -1500, bottom: 500 }, 1000), 1)
  assert.equal(readingProgress({ left: 0, right: 0, top: 100, bottom: 600 }, 1000), 1)
})

test('字号和行距一档一档挪，存下的值只认档位', () => {
  assert.equal(stepFrom(SIZE_STEPS, 100, 1), 110)
  assert.equal(stepFrom(SIZE_STEPS, 100, -1), 95)
  assert.equal(stepFrom(SIZE_STEPS, 135, 1), 135)
  assert.equal(stepFrom(LEADING_STEPS, 1.82, 1), 1.9)
  assert.equal(stepFrom(LEADING_STEPS, 1.82, -1), 1.75)
  assert.deepEqual(readingPrefsFromValue({ size: 110, leading: 1.9 }), { size: 110, leading: 1.9 })
  assert.deepEqual(readingPrefsFromValue({ size: 103, leading: '2' }), { size: null, leading: null })
  assert.deepEqual(readingPrefsFromValue(null), { size: null, leading: null })
})
