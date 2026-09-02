import assert from 'node:assert/strict'
import test from 'node:test'
import { toggleImmersiveReason } from './NavigationContext'

test('单个理由进出，chrome 跟着藏和回', () => {
  const reasons = new Set<string>()
  assert.equal(toggleImmersiveReason(reasons, 'brew-reader', true), true)
  assert.equal(toggleImmersiveReason(reasons, 'brew-reader', false), false)
})

test('两方同时沉浸时，先退出的一方不会替另一方把 chrome 放出来', () => {
  const reasons = new Set<string>()
  // Tapp 全屏中，用户唤起 Agent 岛
  toggleImmersiveReason(reasons, 'tapp-fullscreen', true)
  toggleImmersiveReason(reasons, 'agent-panel', true)

  // 收起 Agent 岛 —— 全屏还在，导航岛不该冒出来
  assert.equal(toggleImmersiveReason(reasons, 'agent-panel', false), true)

  // 退出全屏，最后一个理由撤销，这才恢复
  assert.equal(toggleImmersiveReason(reasons, 'tapp-fullscreen', false), false)
})

test('重复撤销同一个理由不会把别人的理由一起清掉', () => {
  const reasons = new Set<string>()
  toggleImmersiveReason(reasons, 'a', true)
  toggleImmersiveReason(reasons, 'b', true)
  toggleImmersiveReason(reasons, 'a', false)
  assert.equal(toggleImmersiveReason(reasons, 'a', false), true)
  assert.equal(reasons.size, 1)
})

test('同一个 hook 挂多份互不顶掉 —— 实例后缀让理由各归各', () => {
  const reasons = new Set<string>()
  toggleImmersiveReason(reasons, 'tapp-fullscreen#r1', true)
  toggleImmersiveReason(reasons, 'tapp-fullscreen#r2', true)
  assert.equal(toggleImmersiveReason(reasons, 'tapp-fullscreen#r1', false), true)
  assert.equal(toggleImmersiveReason(reasons, 'tapp-fullscreen#r2', false), false)
})
