import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  classifyWidgetLibraryKind,
  collectWidgetLibrarySearchText,
  normalizeWidgetLibraryQuery,
  presentWidgetLibraryKindFilters,
  widgetMatchesLibraryKind,
  widgetMatchesLibrarySearch,
  widgetTypeMatchesLibrarySearch,
} from './widgetLibrarySearch'

describe('widgetLibrarySearch', () => {
  it('normalizes query (trim + lower-case)', () => {
    assert.equal(normalizeWidgetLibraryQuery('  Weather  '), 'weather')
  })

  it('matches empty query against everything', () => {
    assert.equal(widgetMatchesLibrarySearch('', ['Welcome']), true)
    assert.equal(widgetMatchesLibrarySearch('   ', ['Welcome']), true)
  })

  it('matches runtime name/id without any preset map', () => {
    assert.equal(
      widgetTypeMatchesLibrarySearch('clock', {
        id: 'com.example.world-clock',
        name: 'World Clock',
      }),
      true,
    )
    assert.equal(
      widgetTypeMatchesLibrarySearch('world', {
        id: 'com.example.world-clock',
        name: 'World Clock',
      }),
      true,
    )
    assert.equal(
      widgetTypeMatchesLibrarySearch('example', {
        id: 'com.example.world-clock',
        name: 'World Clock',
      }),
      true,
    )
  })

  it('matches optional label and free-form extras (Tapp category / tappId)', () => {
    assert.equal(
      widgetTypeMatchesLibrarySearch('天气', {
        id: 'weather',
        name: 'Weather',
        label: '天气',
      }),
      true,
    )
    assert.equal(
      widgetTypeMatchesLibrarySearch('productivity', {
        id: 'todo.list',
        name: 'Todos',
        extras: ['productivity', 'tapp-my-todo'],
      }),
      true,
    )
    assert.equal(
      widgetTypeMatchesLibrarySearch('my-todo', {
        id: 'todo.list',
        name: 'Todos',
        extras: ['productivity', 'tapp-my-todo'],
      }),
      true,
    )
  })

  it('matches id with separators via spaced form', () => {
    const text = collectWidgetLibrarySearchText({
      id: 'music-player',
      name: 'Music',
    })
    assert.ok(text.includes('music-player'))
    assert.ok(text.includes('music player'))
    assert.equal(
      widgetTypeMatchesLibrarySearch('music player', {
        id: 'music-player',
        name: 'Music',
      }),
      true,
    )
  })

  it('rejects non-matching query', () => {
    assert.equal(
      widgetTypeMatchesLibrarySearch('github', {
        id: 'weather',
        name: 'Weather',
        label: '天气',
      }),
      false,
    )
  })

  it('ignores blank candidates', () => {
    assert.equal(widgetMatchesLibrarySearch('x', [null, undefined, '  ']), false)
    assert.equal(widgetMatchesLibrarySearch('x', [null, 'axb']), true)
  })

  it('classifies host widgets onto topic rows with Tapp', () => {
    assert.equal(classifyWidgetLibraryKind({ id: 'weather' }), 'tapp:utility')
    assert.equal(classifyWidgetLibraryKind({ id: 'music-player' }), 'tapp:media')
    assert.equal(classifyWidgetLibraryKind({ id: 'report-github' }), 'report')
    assert.equal(
      classifyWidgetLibraryKind({
        id: 'com.example.clock',
        isTappWidget: true,
        category: 'media',
      }),
      'tapp:media',
    )
    assert.equal(
      classifyWidgetLibraryKind({
        id: 'com.example.todo',
        isTappWidget: true,
      }),
      'tapp:utility',
    )
  })

  it('lists only kinds that currently have a widget', () => {
    assert.deepEqual(
      presentWidgetLibraryKindFilters([
        { id: 'weather' },
        { id: 'music-player' },
        { id: 'report-github' },
        {
          id: 'com.example.clock',
          isTappWidget: true,
          category: 'media',
        },
        {
          id: 'com.example.notes',
          isTappWidget: true,
          category: 'productivity',
        },
      ]),
      ['all', 'report', 'tapp:media', 'tapp:productivity', 'tapp:utility'],
    )
  })

  it('omits empty report rows', () => {
    assert.deepEqual(presentWidgetLibraryKindFilters([{ id: 'weather' }]), [
      'all',
      'tapp:utility',
    ])
  })

  it('matches kind filter', () => {
    assert.equal(widgetMatchesLibraryKind('all', { id: 'weather' }), true)
    assert.equal(
      widgetMatchesLibraryKind('tapp:utility', { id: 'weather' }),
      true,
    )
    assert.equal(
      widgetMatchesLibraryKind('report', { id: 'report-github' }),
      true,
    )
    assert.equal(
      widgetMatchesLibraryKind('tapp:media', {
        id: 'com.example.clock',
        isTappWidget: true,
        category: 'media',
      }),
      true,
    )
    assert.equal(
      widgetMatchesLibraryKind('tapp:media', { id: 'music-player' }),
      true,
    )
    assert.equal(
      widgetMatchesLibraryKind('tapp:media', { id: 'weather' }),
      false,
    )
  })
})
