import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import React from 'react'
import ts from 'typescript'

const source = ts.createSourceFile('widgets.tsx', readFileSync(new URL('./useTappWidgets.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
const names = new Set(['createTappWidgetType', 'resolveTappAccent', 'mapTappSize', 'mapTappSizes'])
const body = source.statements.filter(node => ts.isFunctionDeclaration(node) && names.has(node.name?.text || '')).map(node => node.getText(source)).join('\n')
const deps = { createElement: React.createElement, Suspense: React.Suspense, TappDefaultSkeleton: () => null, TappWidget: () => null, TappWidgetPopup: () => null, TAPP_SIZE_MAP: { '2x2': '2x2' } }
const factory = compileFunction(`${ts.transpile(body)}; return createTappWidgetType;`, Object.keys(deps))(...Object.values(deps))

test('registry metadata refresh preserves React component identity and updates metadata', () => {
  const original = factory({ id: 'test.card', tappId: 'test', config: { name: 'Old' } })
  const refreshed = factory({ id: 'test.card', tappId: 'test', config: { name: 'New' } }, undefined, { component: original.component })
  assert.equal(refreshed.component, original.component)
  assert.equal(refreshed.name, 'New')
  assert.equal(refreshed.component({}).props.children.props.tappWidgetId, 'test.card')
  assert.equal(refreshed.popup, undefined, 'no popup layer declared, no popup')
})

test('a declared popup layer becomes a popup whose component survives metadata refresh', () => {
  const runtime = { getTapp: () => ({ manifest: { widgets: [{ id: 'card', popup: { entry: 'popup.js', size: 'stage' } }] } }) }
  const original = factory({ id: 'test.card', tappId: 'test', config: { id: 'card', name: 'Old' } }, runtime)
  assert.equal(original.popup.size, 'stage')
  assert.equal(original.popup.chrome, 'close', 'TAPP popups always keep the host close button')
  const refreshed = factory({ id: 'test.card', tappId: 'test', config: { id: 'card', name: 'New' } }, runtime, {
    component: original.component,
    popupComponent: original.popup.component,
  })
  assert.equal(refreshed.popup.component, original.popup.component)
  assert.equal(refreshed.popup.component({}).props.children.props.tappWidgetId, 'test.card')
})

test('lazy-loads the default TappWidget export so the tapp namespace boundary stays', () => {
  const raw = readFileSync(new URL('./useTappWidgets.ts', import.meta.url), 'utf8')
  assert.match(raw, /lazy\(\(\) => import\('\.\.\/components\/widgets\/TappWidget'\)\)/)
  assert.doesNotMatch(raw, /m\.TappWidgetComponent/)
})
