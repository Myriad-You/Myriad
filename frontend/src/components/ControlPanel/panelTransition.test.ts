/**
 * 智能岛展开/收起状态机的单元测试（issue #320）。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/ControlPanel/panelTransition.test.ts
 */

import type { PanelAction, PanelState } from './panelTransition.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  initialPanelState,
  isPanelMorphing,
  isPanelOpen,
  mountsNotifications,
  PANEL_MORPH_BASE_MS,

  panelReducer,

  resolvePanelMotion,
  settleTimeoutMs,
  showsDynamicContent,
  showsOverlay,
  showsOverlayBlur,
  showsPanelContent,
  showsProgressUi,
} from './panelTransition.ts'

/** 依次施加动作，返回终态。 */
function run(state: PanelState, ...actions: PanelAction[]): PanelState {
  return actions.reduce(panelReducer, state)
}

/** 完成一次 morph：用当前世代号 settle。 */
function settle(state: PanelState): PanelState {
  return panelReducer(state, { type: 'settle', generation: state.generation })
}

describe('相位推进', () => {
  it('展开走 collapsed → opening → expanded', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    assert.equal(opening.phase, 'opening')
    assert.equal(settle(opening).phase, 'expanded')
  })

  it('收起走 expanded → closing → collapsed', () => {
    const expanded = settle(panelReducer(initialPanelState, { type: 'open' }))
    const closing = panelReducer(expanded, { type: 'close' })
    assert.equal(closing.phase, 'closing')
    assert.equal(settle(closing).phase, 'collapsed')
  })

  it('每次相位切换都推进世代号', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    const expanded = settle(opening)
    const closing = panelReducer(expanded, { type: 'close' })
    const collapsed = settle(closing)
    assert.deepEqual(
      [opening.generation, expanded.generation, closing.generation, collapsed.generation],
      [1, 2, 3, 4],
    )
  })

  it('重复 open / close 不重启动画，返回同一对象', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    assert.equal(panelReducer(opening, { type: 'open' }), opening)

    const expanded = settle(opening)
    assert.equal(panelReducer(expanded, { type: 'open' }), expanded)

    const closing = panelReducer(expanded, { type: 'close' })
    assert.equal(panelReducer(closing, { type: 'close' }), closing)
    assert.equal(panelReducer(initialPanelState, { type: 'close' }), initialPanelState)
  })
})

describe('过期回调作废（快速连点）', () => {
  it('旧世代的 settle 不会推进新动画', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    const staleGen = opening.generation
    // 展开动画还没结束就被关掉
    const closing = panelReducer(opening, { type: 'close' })
    // 上一次展开的 transitionend / 超时迟到
    const after = panelReducer(closing, { type: 'settle', generation: staleGen })
    assert.equal(after, closing)
    assert.equal(after.phase, 'closing')
  })

  it('open→close→open 后停在 opening，且只认最新世代', () => {
    const s = run(
      initialPanelState,
      { type: 'open' },
      { type: 'close' },
      { type: 'open' },
    )
    assert.equal(s.phase, 'opening')
    assert.equal(panelReducer(s, { type: 'settle', generation: 1 }), s)
    assert.equal(panelReducer(s, { type: 'settle', generation: 2 }), s)
    assert.equal(settle(s).phase, 'expanded')
  })

  it('settle 后再次 settle 同一世代不会二次推进', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    const expanded = settle(opening)
    // transitionend 与兜底超时同时到达
    const again = panelReducer(expanded, {
      type: 'settle',
      generation: opening.generation,
    })
    assert.equal(again, expanded)
    assert.equal(again.phase, 'expanded')
  })

  it('收起结束一定回到完全干净的初态（不残留 tab / 通知挂载 / 哨兵）', () => {
    const s = run(
      initialPanelState,
      { type: 'open', tab: 'notifications', historyArmed: true },
    )
    const expanded = settle(s)
    const collapsed = settle(panelReducer(expanded, { type: 'close' }))
    assert.equal(collapsed.phase, 'collapsed')
    assert.equal(collapsed.tab, 'control')
    assert.equal(collapsed.notifMounted, false)
    assert.equal(collapsed.historyArmed, false)
  })
})

describe('哨兵历史记录', () => {
  it('展开时按调用方结果记录，收起时立即解除武装', () => {
    const opening = panelReducer(initialPanelState, {
      type: 'open',
      historyArmed: true,
    })
    assert.equal(opening.historyArmed, true)
    const closing = panelReducer(settle(opening), { type: 'close' })
    assert.equal(closing.historyArmed, false)
  })

  it('pushState 失败时不武装（避免收起时误吞一次返回）', () => {
    const opening = panelReducer(initialPanelState, {
      type: 'open',
      historyArmed: false,
    })
    assert.equal(opening.historyArmed, false)
  })

  it('展开中重复 open 不会重复武装', () => {
    const opening = panelReducer(initialPanelState, {
      type: 'open',
      historyArmed: true,
    })
    const again = panelReducer(opening, { type: 'open', historyArmed: true })
    assert.equal(again, opening)
  })
})

describe('tab 切换', () => {
  it('切到通知页后保持挂载，往返都是同一表面的交叉淡入', () => {
    const expanded = settle(panelReducer(initialPanelState, { type: 'open' }))
    assert.equal(mountsNotifications(expanded), false)

    const onNotif = panelReducer(expanded, {
      type: 'selectTab',
      tab: 'notifications',
    })
    assert.equal(onNotif.tab, 'notifications')
    assert.equal(mountsNotifications(onNotif), true)

    const back = panelReducer(onNotif, { type: 'selectTab', tab: 'control' })
    assert.equal(back.tab, 'control')
    // 淡出中的旧表面仍需在 DOM 里
    assert.equal(mountsNotifications(back), true)
  })

  it('tab 切换不推进世代号（不触发外壳重新 morph）', () => {
    const expanded = settle(panelReducer(initialPanelState, { type: 'open' }))
    const onNotif = panelReducer(expanded, {
      type: 'selectTab',
      tab: 'notifications',
    })
    assert.equal(onNotif.generation, expanded.generation)
    assert.equal(onNotif.phase, 'expanded')
  })

  it('展开动画中即可切 tab', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    const onNotif = panelReducer(opening, {
      type: 'selectTab',
      tab: 'notifications',
    })
    assert.equal(onNotif.tab, 'notifications')
    assert.equal(onNotif.phase, 'opening')
  })

  it('收起中 / 已收起时不再改 tab（收起动画不发生内容硬切）', () => {
    const expanded = settle(panelReducer(initialPanelState, { type: 'open' }))
    const closing = panelReducer(expanded, { type: 'close' })
    assert.equal(
      panelReducer(closing, { type: 'selectTab', tab: 'notifications' }),
      closing,
    )
    assert.equal(
      panelReducer(initialPanelState, { type: 'selectTab', tab: 'notifications' }),
      initialPanelState,
    )
  })

  it('open 可直达通知页（点轮播里的通知）', () => {
    const s = panelReducer(initialPanelState, {
      type: 'open',
      tab: 'notifications',
    })
    assert.equal(s.tab, 'notifications')
    assert.equal(mountsNotifications(s), true)
  })

  it('收起后重新展开默认回到控制页', () => {
    const collapsed = settle(
      panelReducer(
        settle(panelReducer(initialPanelState, { type: 'open', tab: 'notifications' })),
        { type: 'close' },
      ),
    )
    assert.equal(panelReducer(collapsed, { type: 'open' }).tab, 'control')
  })
})

describe('派生可见性', () => {
  const opening = panelReducer(initialPanelState, { type: 'open' })
  const expanded = settle(opening)
  const closing = panelReducer(expanded, { type: 'close' })

  it('展开内容与遮罩在 opening 首帧即参与渲染（无固定空壳窗口）', () => {
    assert.equal(showsPanelContent(opening), true)
    assert.equal(showsOverlay(opening), true)
    assert.equal(isPanelOpen(opening), true)
  })

  it('收缩内容在 closing 首帧即开始淡回', () => {
    assert.equal(showsDynamicContent(closing), true)
    assert.equal(showsPanelContent(closing), false)
    assert.equal(showsOverlay(closing), false)
  })

  it('收缩内容与展开内容不会同时被判定为主表面', () => {
    for (const s of [initialPanelState, opening, expanded, closing]) {
      assert.equal(
        showsDynamicContent(s) && showsPanelContent(s),
        false,
        `phase=${s.phase}`,
      )
    }
  })

  it('morph 标记只在两段动画期间为真', () => {
    assert.equal(isPanelMorphing(opening), true)
    assert.equal(isPanelMorphing(closing), true)
    assert.equal(isPanelMorphing(expanded), false)
    assert.equal(isPanelMorphing(initialPanelState), false)
  })

  it('进度 UI 只在控制页可见时开', () => {
    assert.equal(showsProgressUi(expanded), true)
    assert.equal(
      showsProgressUi(panelReducer(expanded, { type: 'selectTab', tab: 'notifications' })),
      false,
    )
    assert.equal(showsProgressUi(closing), false)
    assert.equal(showsProgressUi(initialPanelState), false)
  })
})

describe('动效档位', () => {
  const standard = resolvePanelMotion({
    level: 'standard',
    reduceMotion: false,
    isMobile: false,
  })

  it('桌面标准档保持既有 700ms 观感并保留 morph 期间模糊', () => {
    assert.equal(standard.morphMs, PANEL_MORPH_BASE_MS)
    assert.equal(standard.spatial, true)
    assert.equal(standard.blurDuringMorph, true)
  })

  it('移动端标准档不在 morph 热路径上做全屏模糊', () => {
    const mobile = resolvePanelMotion({
      level: 'standard',
      reduceMotion: false,
      isMobile: true,
    })
    assert.equal(mobile.morphMs, PANEL_MORPH_BASE_MS)
    assert.equal(mobile.blurDuringMorph, false)
  })

  it('prefers-reduced-motion 得到短促的非空间过渡', () => {
    const reduced = resolvePanelMotion({
      level: 'standard',
      reduceMotion: true,
      isMobile: false,
    })
    assert.equal(reduced.spatial, false)
    assert.equal(reduced.blurDuringMorph, false)
    assert.ok(reduced.morphMs <= 150, `morphMs=${reduced.morphMs}`)
  })

  it('低性能档缩短 morph 且不做昂贵模糊', () => {
    const light = resolvePanelMotion({
      level: 'light',
      reduceMotion: false,
      isMobile: false,
    })
    assert.ok(light.morphMs < standard.morphMs)
    assert.equal(light.spatial, true)
    assert.equal(light.blurDuringMorph, false)

    const exlight = resolvePanelMotion({
      level: 'exlight',
      reduceMotion: false,
      isMobile: false,
    })
    assert.equal(exlight.spatial, false)
    assert.equal(exlight.blurDuringMorph, false)
  })

  it('遮罩模糊在非标准档推迟到稳定展开态', () => {
    const opening = panelReducer(initialPanelState, { type: 'open' })
    const expanded = settle(opening)
    const mobile = resolvePanelMotion({
      level: 'standard',
      reduceMotion: false,
      isMobile: true,
    })
    assert.equal(showsOverlayBlur(opening, mobile), false)
    assert.equal(showsOverlayBlur(expanded, mobile), true)
    // 桌面标准档与既有观感一致：随遮罩一起出现
    assert.equal(showsOverlayBlur(opening, standard), true)
  })

  it('兜底超时始终晚于 morph 本身', () => {
    for (const level of ['standard', 'light', 'exlight'] as const) {
      const m = resolvePanelMotion({ level, reduceMotion: false, isMobile: false })
      assert.ok(settleTimeoutMs(m) > m.morphMs, level)
    }
  })
})
