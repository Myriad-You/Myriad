import assert from 'node:assert/strict'
import { it } from 'node:test'
import {
  articleSwapStale,
  completeSiteView,
  idleSitesIntent,
  requestSiteView,
} from './feedsMotion'

it('keeps the latest expand/fold and ignores clicks already heading there', () => {
  let state = idleSitesIntent(false)
  const open = requestSiteView(state, true)
  assert.equal(open.action, 'flip')
  state = open.state
  assert.equal(state.targetOpen, true)
  assert.equal(state.displayOpen, true)
  assert.equal(state.flipping, true)
  assert.equal(requestSiteView(state, true).action, 'noop')

  const fold = requestSiteView(state, false)
  assert.equal(fold.action, 'retarget')
  state = fold.state
  assert.equal(state.targetOpen, false)
  assert.equal(state.generation, 2)

  const back = requestSiteView(state, true)
  assert.equal(back.action, 'retarget')
  assert.equal(back.state.generation, 3)
  assert.equal(back.state.targetOpen, true)
})

it('drops a stale completion after retarget and idles on the matching generation', () => {
  const opened = requestSiteView(idleSitesIntent(false), true)
  const started = opened.state.generation
  const folded = requestSiteView(opened.state, false)
  assert.equal(folded.action, 'retarget')
  assert.equal(completeSiteView(folded.state, started).action, 'ignore')
  assert.equal(completeSiteView(folded.state, folded.state.generation).action, 'idle')
})

it('does not let an old article swap generation commit', () => {
  assert.equal(articleSwapStale(1, 1), false)
  assert.equal(articleSwapStale(1, 2), true)
})
