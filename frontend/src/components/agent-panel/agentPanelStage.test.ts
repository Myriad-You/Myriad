import assert from 'node:assert/strict'
import test from 'node:test'
import {
  AGENT_PANEL_EXIT_MS,
  AGENT_PANEL_SETTLE_SLACK_MS,
  AGENT_ROW_EXIT_MS,
  AGENT_ROW_STAGGER_MAX,
  AGENT_ROW_STAGGER_MS,
  agentPanelIsOpen,
  agentPanelRowWaveMs,
  agentPanelSettleTimeoutMs,
  agentPanelShowsStage,
  agentPanelStaggerSteps,
  INITIAL_AGENT_PANEL_STAGE as start,
  agentPanelStageReducer as step,
} from './agentPanelStage'

test('长按展开，动画播完才算落定', () => {
  const opening = step(start, { type: 'open', stage: 'overlay' })
  assert.deepEqual(opening, { stage: 'overlay', phase: 'opening' })
  assert.equal(agentPanelIsOpen(opening), true)

  const open = step(opening, { type: 'settle' })
  assert.deepEqual(open, { stage: 'overlay', phase: 'settled' })
})

test('收起时内容留到动画播完，不硬切', () => {
  const open = step(step(start, { type: 'open', stage: 'overlay' }), {
    type: 'settle',
  })

  const closing = step(open, { type: 'close' })
  assert.deepEqual(closing, { stage: 'overlay', phase: 'closing' })
  assert.equal(agentPanelShowsStage(closing, 'overlay'), true)
  assert.equal(agentPanelIsOpen(closing), false)

  assert.deepEqual(step(closing, { type: 'settle' }), start)
})

test('重复长按同一档不重播动画', () => {
  const open = step(step(start, { type: 'open', stage: 'overlay' }), {
    type: 'settle',
  })
  assert.equal(step(open, { type: 'open', stage: 'overlay' }), open)
})

test('收起动画途中再展开，直接回到展开', () => {
  const open = step(step(start, { type: 'open', stage: 'overlay' }), {
    type: 'settle',
  })
  const closing = step(open, { type: 'close' })
  const reopened = step(closing, { type: 'open', stage: 'overlay' })
  assert.deepEqual(reopened, { stage: 'overlay', phase: 'opening' })
})

test('toggle 在同一档上开合，跨档则直接换档', () => {
  const open = step(step(start, { type: 'open', stage: 'overlay' }), {
    type: 'settle',
  })
  assert.deepEqual(step(open, { type: 'toggle', stage: 'overlay' }), {
    stage: 'overlay',
    phase: 'closing',
  })
  assert.deepEqual(step(open, { type: 'toggle', stage: 'full' }), {
    stage: 'full',
    phase: 'opening',
  })
})

test('已经在岛上时收起是空操作，settle 也不会乱改', () => {
  assert.equal(step(start, { type: 'close' }), start)
  assert.equal(step(start, { type: 'settle' }), start)
})

test('丢了动画事件也能靠超时把状态推回落定', () => {
  const opening = step(start, { type: 'open', stage: 'overlay' })
  assert.equal(step(opening, { type: 'settle' }).phase, 'settled')
})

test('展开对话时收起要等逐张动画走完', () => {
  assert.equal(
    agentPanelSettleTimeoutMs('overlay'),
    AGENT_PANEL_EXIT_MS + AGENT_PANEL_SETTLE_SLACK_MS,
  )
  assert.equal(agentPanelStaggerSteps(0), 0)
  assert.equal(agentPanelStaggerSteps(2), 2)
  assert.equal(agentPanelStaggerSteps(20), AGENT_ROW_STAGGER_MAX + 1)
  assert.equal(
    agentPanelRowWaveMs(),
    AGENT_ROW_EXIT_MS + AGENT_ROW_STAGGER_MS * (AGENT_ROW_STAGGER_MAX + 1),
  )
  assert.equal(
    agentPanelRowWaveMs(2),
    AGENT_ROW_EXIT_MS + AGENT_ROW_STAGGER_MS * 2,
  )
  assert.equal(
    agentPanelSettleTimeoutMs('full'),
    AGENT_ROW_EXIT_MS +
      AGENT_ROW_STAGGER_MS * (AGENT_ROW_STAGGER_MAX + 2) +
      AGENT_PANEL_SETTLE_SLACK_MS,
  )
  assert.equal(
    agentPanelSettleTimeoutMs('full', 2),
    AGENT_ROW_EXIT_MS + AGENT_ROW_STAGGER_MS * 3 + AGENT_PANEL_SETTLE_SLACK_MS,
  )
  assert.ok(agentPanelSettleTimeoutMs('full') > agentPanelRowWaveMs())
})
