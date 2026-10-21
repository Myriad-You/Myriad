import type { WidgetType } from '../components/widgetGridTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { beginTappSubjectChange, finishTappSubjectChange } from '../utils/tappSubject'
import {
  closeWidgetPopup,
  dropWidgetPopups,
  getWidgetPopupState,
  isWidgetPopupHostArmed,
  openWidgetPopup,
  WIDGET_POPUP_STASH_LIMIT,
} from './widgetPopup'

const withPopup = { id: 'report-x', name: 'X', defaultSize: '4x2', component: () => null, popup: { component: () => null, size: 'stage' } } as unknown as WidgetType
const noPopup = { ...withPopup, popup: undefined } as WidgetType

function open(id: string, params?: unknown, type = withPopup) {
  openWidgetPopup({
    widget: { id, type: type.id, size: '4x2', position: { x: 0, y: 0 } },
    widgetType: type,
    params,
    anchor: () => null,
    returnFocus: () => null,
  })
}

test('a widget without a popup cannot open one', () => {
  dropWidgetPopups()
  open('plain', undefined, noPopup)
  assert.equal(getWidgetPopupState().entries.length, 0)
})

test('closing stashes instead of destroying, and reopening keeps earlier params', () => {
  dropWidgetPopups()
  open('a', { platform: 'steam' })
  assert.ok(isWidgetPopupHostArmed())
  assert.equal(getWidgetPopupState().activeKey, 'a')
  assert.equal(getWidgetPopupState().entries[0].size, 'stage')
  closeWidgetPopup()
  assert.equal(getWidgetPopupState().activeKey, null)
  assert.equal(getWidgetPopupState().entries.length, 1, 'still mounted while stashed')
  open('a')
  assert.deepEqual(getWidgetPopupState().entries[0].params, { platform: 'steam' })
  closeWidgetPopup()
  open('a', null)
  assert.equal(getWidgetPopupState().entries[0].params, null, 'an explicit null clears the params')
})

test('close with a key only closes that popup', () => {
  dropWidgetPopups()
  open('a')
  closeWidgetPopup('b')
  assert.equal(getWidgetPopupState().activeKey, 'a')
})

test('the stash keeps the most recently used popups only', () => {
  dropWidgetPopups()
  for (let i = 0; i < WIDGET_POPUP_STASH_LIMIT + 2; i++) open(`w${i}`)
  const { entries, activeKey } = getWidgetPopupState()
  assert.equal(activeKey, `w${WIDGET_POPUP_STASH_LIMIT + 1}`)
  assert.equal(entries.length, WIDGET_POPUP_STASH_LIMIT + 1)
  assert.ok(!entries.some((entry) => entry.key === 'w0'), 'oldest stash evicted')
})

test('dropping destroys stashed and active popups that match', () => {
  dropWidgetPopups()
  open('a')
  open('b')
  dropWidgetPopups((entry) => entry.key === 'b')
  assert.equal(getWidgetPopupState().activeKey, null)
  assert.deepEqual(getWidgetPopupState().entries.map((entry) => entry.key), ['a'])
})

test('an identity change destroys every popup before anything re-renders', () => {
  dropWidgetPopups()
  open('a')
  closeWidgetPopup()
  open('b')
  const epoch = beginTappSubjectChange()
  assert.deepEqual(getWidgetPopupState(), { entries: [], activeKey: null })
  finishTappSubjectChange(epoch, false)
})

test('cards sharing a popup reuse one instance and hand it their own params', () => {
  dropWidgetPopups()
  const shared = { ...withPopup, popup: { ...withPopup.popup!, shareKey: 'stage' } } as WidgetType
  open('steam-card', { platform: 'steam' }, shared)
  closeWidgetPopup()
  open('github-card', { platform: 'github' }, shared)
  const { entries, activeKey } = getWidgetPopupState()
  assert.equal(entries.length, 1)
  assert.equal(activeKey, 'stage')
  assert.equal(entries[0].widget.id, 'github-card', 'anchors to the card that opened it last')
  assert.deepEqual(entries[0].params, { platform: 'github' })
  dropWidgetPopups((entry) => entry.widget.id === 'github-card')
  assert.equal(getWidgetPopupState().entries.length, 0, 'deleting that card drops the shared popup')
})
