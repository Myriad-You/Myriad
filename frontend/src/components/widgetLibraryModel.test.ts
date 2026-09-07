import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  widgetDisplayLabel,
  widgetHostConfig,
  widgetLibraryKindSource,
  widgetPreviewConfig,
  widgetSearchExtras,
  widgetTranslationKey,
} from './widgetLibraryModel'

describe('widgetHostConfig', () => {
  it('lifts platform and report ids, and ignores host widgets', () => {
    assert.deepEqual(widgetHostConfig('platform-github'), {
      platformId: 'github',
    })
    assert.deepEqual(widgetHostConfig('report-bilibili'), {
      platformId: 'bilibili',
    })
    assert.equal(widgetHostConfig('weather'), undefined)
  })
})

describe('widgetPreviewConfig', () => {
  it('builds a catalog preview config from the type', () => {
    const config = widgetPreviewConfig({
      id: 'platform-github',
      defaultSize: '2x2',
    })
    assert.equal(config.id, 'preview-platform-github')
    assert.equal(config.type, 'platform-github')
    assert.equal(config.size, '2x2')
    assert.deepEqual(config.config, { platformId: 'github' })
  })
})

describe('catalog metadata', () => {
  it('reads Tapp fields from WidgetType without casting', () => {
    const widget = {
      id: 'tapp.clock',
      name: 'Clock',
      isTappWidget: true,
      tappId: 'clock.tapp',
      category: 'utility',
      description: 'A clock',
    }
    assert.deepEqual(widgetLibraryKindSource(widget), {
      id: 'tapp.clock',
      isTappWidget: true,
      category: 'utility',
    })
    assert.deepEqual(widgetSearchExtras(widget), [
      'utility',
      'clock.tapp',
      'A clock',
    ])
  })
})

describe('widgetDisplayLabel', () => {
  it('prefers i18n then falls back to the registry name', () => {
    assert.equal(widgetTranslationKey('music-player'), 'musicPlayer')
    assert.equal(
      widgetDisplayLabel(
        { id: 'music-player', name: 'Music' },
        { musicPlayer: '音乐' },
      ),
      '音乐',
    )
    assert.equal(
      widgetDisplayLabel({ id: 'music-player', name: 'Music' }, {}),
      'Music',
    )
  })
})
