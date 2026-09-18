import type { TranslationKeys } from '../../i18n'
import assert from 'node:assert/strict'
import test from 'node:test'
import { BUILTIN_WIDGET_BASE_CONFIG, getBuiltinWidgets } from './builtinWidgets'

const widgetsI18n = {
  agentPersona: 'Agent Persona',
} as TranslationKeys['widgets']

test('agent persona widget supports 2x2 and 4x4', () => {
  const config = BUILTIN_WIDGET_BASE_CONFIG['agent-persona']
  assert.equal(config.defaultSize, '2x2')
  assert.deepEqual(config.supportedSizes, ['2x2', '4x4'])
})

test('github repos widget is in the home catalog', () => {
  const homeIds = getBuiltinWidgets(widgetsI18n, 'home').map(({ id }) => id)
  assert.ok(homeIds.includes('github-repos'))
  const config = BUILTIN_WIDGET_BASE_CONFIG['github-repos']
  assert.equal(config.defaultSize, '2x2')
})

test('component-owned settings defer drag for their long press', () => {
  const widgets = getBuiltinWidgets(widgetsI18n, 'home')
  const byId = new Map(widgets.map((widget) => [widget.id, widget]))

  for (const id of [
    'social-network',
    'tapp-shortcut',
    'game-presence',
    'report-github',
  ]) {
    assert.equal(byId.get(id)?.componentLongPress, true, id)
  }
  assert.equal(byId.get('weather')?.componentLongPress, undefined)
  assert.equal(byId.get('github-repos')?.componentLongPress, undefined)
})

test('agent persona widget is available on Home only', () => {
  const homeIds = getBuiltinWidgets(widgetsI18n, 'home').map(({ id }) => id)
  const controlPanelIds = getBuiltinWidgets(widgetsI18n, 'control-panel').map(
    ({ id }) => id,
  )

  assert.ok(homeIds.includes('agent-persona'))
  assert.ok(!controlPanelIds.includes('agent-persona'))
})

test('note catalog uses hosts, not a special-case blacklist', () => {
  const noteIds = getBuiltinWidgets(widgetsI18n, 'note').map(({ id }) => id)
  assert.ok(noteIds.includes('weather'))
  assert.ok(noteIds.includes('friend-links'))
  assert.ok(noteIds.includes('report-bilibili'))
  assert.ok(noteIds.includes('tapp-shortcut'))
  assert.ok(!noteIds.includes('welcome'))
  assert.ok(!noteIds.includes('agent-persona'))
  assert.ok(!noteIds.includes('phantasi-featured'))
  const homeIds = getBuiltinWidgets(widgetsI18n, 'home').map(({ id }) => id)
  const panelIds = getBuiltinWidgets(widgetsI18n, 'control-panel').map(({ id }) => id)
  assert.ok(homeIds.includes('welcome'))
  assert.ok(homeIds.includes('friend-links'))
  assert.ok(homeIds.includes('phantasi-featured'))
  assert.ok(!homeIds.includes('phantasi-source'))
  assert.ok(!homeIds.includes('phantasi-topic'))
  assert.ok(panelIds.includes('welcome'))
  assert.ok(!panelIds.includes('friend-links'))
  assert.ok(!panelIds.includes('phantasi-featured'))
})
