import type { FrontendAction, FrontendActionType } from '../../services/agent'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ACTION_REVERSIBILITY,
  agentActionReversibility,
  planAgentUndo,
  UNDO_WINDOW_MS,
} from './agentUndo'

const NOW = 1_700_000_000_000

function action(type: FrontendActionType, path?: string): FrontendAction {
  return { type, timestamp: NOW, ...(path ? { path } : {}) }
}

test('每一种前端操作都表过态，没有漏网的', () => {
  const declared: FrontendActionType[] = [
    'query_windows',
    'open_window',
    'close_window',
    'focus_window',
    'agent_interaction',
    'navigate',
    'page_interact',
    'brew_open_article',
    'music_control',
    'music_get_status',
    'music_load_playlist',
    'reading_list',
    'show_notification',
    'copy_clipboard',
    'play_audio',
    'show_data',
    'download_file',
    'show_report',
  ]
  for (const type of declared) {
    assert.ok(ACTION_REVERSIBILITY[type], `${type} 没有分类`)
  }
  assert.equal(Object.keys(ACTION_REVERSIBILITY).length, declared.length)
})

test('认不出来的类型按退不回去算', () => {
  assert.equal(
    agentActionReversibility('something_new' as FrontendActionType),
    'irreversible',
  )
})

test('换过路由的操作能原路退回', () => {
  const offer = planAgentUndo({
    action: action('navigate', '/library'),
    beforePath: '/brew?tag=ai',
    afterPath: '/library',
    nowMs: NOW,
  })

  assert.ok(offer)
  assert.equal(offer.actionType, 'navigate')
  assert.equal(offer.inverse.type, 'navigate')
  assert.equal(offer.inverse.path, '/brew?tag=ai')
  assert.equal(offer.expiresAtMs, NOW + UNDO_WINDOW_MS)
})

test('打开文章也算换路由，同样能退', () => {
  const offer = planAgentUndo({
    action: action('brew_open_article'),
    beforePath: '/brew',
    afterPath: '/brew/item/42',
    nowMs: NOW,
  })
  assert.equal(offer?.actionType, 'brew_open_article')
  assert.equal(offer?.inverse.path, '/brew')
})

test('路由没真的变过就没有可撤销的东西', () => {
  assert.equal(
    planAgentUndo({
      action: action('navigate', '/brew'),
      beforePath: '/brew',
      afterPath: '/brew',
      nowMs: NOW,
    }),
    null,
  )
})

test('退不回去的操作不给假的撤销', () => {
  for (const type of [
    'page_interact',
    'music_control',
    'reading_list',
    'open_window',
    'close_window',
  ] as FrontendActionType[]) {
    assert.equal(
      planAgentUndo({
        action: action(type),
        beforePath: '/brew',
        afterPath: '/library',
        nowMs: NOW,
      }),
      null,
      `${type} 不该给出撤销`,
    )
  }
})

test('只是问了一句的操作也没有可撤销的东西', () => {
  for (const type of ['query_windows', 'music_get_status'] as const) {
    assert.equal(agentActionReversibility(type), 'readonly')
    assert.equal(
      planAgentUndo({
        action: action(type),
        beforePath: '/brew',
        afterPath: '/library',
        nowMs: NOW,
      }),
      null,
    )
  }
})

test('每次都是新的 id，旧的撤销按不了两遍', () => {
  const first = planAgentUndo({
    action: action('navigate'),
    beforePath: '/a',
    afterPath: '/b',
    nowMs: NOW,
  })
  const second = planAgentUndo({
    action: action('navigate'),
    beforePath: '/b',
    afterPath: '/c',
    nowMs: NOW,
  })
  assert.notEqual(first?.id, second?.id)
})

test('到点之后撤销机会作废', () => {
  const offer = planAgentUndo({
    action: action('navigate'),
    beforePath: '/a',
    afterPath: '/b',
    nowMs: NOW,
  })
  assert.ok(offer)
  assert.equal(offer.expiresAtMs, NOW + UNDO_WINDOW_MS)
})
