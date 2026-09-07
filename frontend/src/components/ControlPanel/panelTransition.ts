/**
 * 智能岛（GlobalControlPanel）展开/收起的唯一状态所有者。
 *
 * 设计约束（见 issue #320）：
 * - 一条时间线、一个状态机：collapsed → opening → expanded → closing → collapsed。
 *   内容可见性、进度 UI、动画类名、哨兵历史全部从 phase 派生，
 *   不再由若干互相独立的 boolean + setTimeout 各自维护。
 * - 相位推进由真实的 transitionend 驱动（组件侧），本模块只负责用
 *   generation 作废过期回调：快速连点时旧动画的 settle 不会打断新动画。
 * - 纯函数、零 DOM / 零 React 依赖，可直接用 node:test 覆盖。
 */

/** 与 `useAnimationLevel` 的 AnimationLevel 结构一致，此处避免反向依赖 React 模块。 */
export type PanelAnimationLevel = 'exlight' | 'light' | 'standard'

export type PanelPhase = 'collapsed' | 'opening' | 'expanded' | 'closing'
export type PanelTab = 'control' | 'notifications'

export interface PanelState {
  phase: PanelPhase
  tab: PanelTab
  /**
   * 通知面板是否已挂载。首次切到通知页后保持挂载，
   * 使 tab 往返成为同一表面上的交叉淡入，而不是卸载/重挂的硬切。
   * 收起到 collapsed 时随 tab 一起复位，回收列表资源。
   */
  notifMounted: boolean
  /** 相位世代号：每次相位切换 +1，用于作废过期的 settle。tab 切换不改变它。 */
  generation: number
  /** 展开时压入的哨兵历史记录是否仍未消费（移动端系统返回先收面板）。 */
  historyArmed: boolean
}

export const initialPanelState: PanelState = {
  phase: 'collapsed',
  tab: 'control',
  notifMounted: false,
  generation: 0,
  historyArmed: false,
}

export type PanelAction =
  /** 用户请求展开；`tab` 用于「点轮播里的通知」这类直达目标页的入口。 */
  | { type: 'open', tab?: PanelTab, historyArmed?: boolean }
  /** 用户请求收起（关闭按钮 / 遮罩 / 系统返回 / 面板内导航）。 */
  | { type: 'close' }
  /** 外壳 morph 真正结束（transitionend 或兜底超时）。 */
  | { type: 'settle', generation: number }
  | { type: 'selectTab', tab: PanelTab }

export function panelReducer(
  state: PanelState,
  action: PanelAction,
): PanelState {
  switch (action.type) {
    case 'open': {
      // 已在展开路径上：不重启 morph、不重复压历史，只允许改 tab
      if (state.phase === 'opening' || state.phase === 'expanded') {
        return action.tab ? selectTab(state, action.tab) : state
      }
      const tab = action.tab ?? 'control'
      return {
        phase: 'opening',
        tab,
        notifMounted: tab === 'notifications',
        generation: state.generation + 1,
        historyArmed: action.historyArmed ?? false,
      }
    }

    case 'close': {
      if (state.phase === 'collapsed' || state.phase === 'closing') {
        return state
      }
      // tab 不在此处复位：收起动画期间仍显示用户当前所在的表面，
      // 到 collapsed 才复位，避免收起首帧的内容硬切。
      return {
        ...state,
        phase: 'closing',
        generation: state.generation + 1,
        historyArmed: false,
      }
    }

    case 'settle': {
      // 过期回调（上一次 morph 的 transitionend / 超时）直接丢弃
      if (action.generation !== state.generation) return state
      if (state.phase === 'opening') {
        return { ...state, phase: 'expanded', generation: state.generation + 1 }
      }
      if (state.phase === 'closing') {
        return { ...initialPanelState, generation: state.generation + 1 }
      }
      return state
    }

    case 'selectTab': {
      if (state.phase === 'collapsed' || state.phase === 'closing') {
        return state
      }
      return selectTab(state, action.tab)
    }

    default:
      return state
  }
}

function selectTab(state: PanelState, tab: PanelTab): PanelState {
  if (state.tab === tab) return state
  return {
    ...state,
    tab,
    notifMounted: state.notifMounted || tab === 'notifications',
  }
}

/* ==========================================================================
   派生选择器 —— 所有可见性都只有这一个来源
   ========================================================================== */

/** 面板处于「展开」语义（含展开动画中）。 */
export function isPanelOpen(state: PanelState): boolean {
  return state.phase === 'opening' || state.phase === 'expanded'
}

/** 外壳 morph 进行中。 */
export function isPanelMorphing(state: PanelState): boolean {
  return state.phase === 'opening' || state.phase === 'closing'
}

/** 展开面板内容参与渲染并淡入（收起阶段交给 CSS 淡出）。 */
export function showsPanelContent(state: PanelState): boolean {
  return isPanelOpen(state)
}

/** 收缩态轮播内容可见（收起阶段即刻开始淡回，与外壳同一条时间线）。 */
export function showsDynamicContent(state: PanelState): boolean {
  return state.phase === 'collapsed' || state.phase === 'closing'
}

/** 透明点击层可见（无视觉遮罩，仅点空白收起）。 */
export function showsOverlay(state: PanelState): boolean {
  return isPanelOpen(state)
}

/** 音乐进度 UI（每秒 tick）只在控制页可见时开。 */
export function showsProgressUi(state: PanelState): boolean {
  return isPanelOpen(state) && state.tab === 'control'
}

/** 通知覆盖层是否参与渲染（含淡出中的旧表面）。 */
export function mountsNotifications(state: PanelState): boolean {
  return state.notifMounted
}

/* ==========================================================================
   动效档位
   ========================================================================== */

export interface PanelMotionProfile {
  /** 外壳 morph 时长（ms）。 */
  morphMs: number
  /** tab 交叉淡入时长（ms）。 */
  tabMs: number
  /** 是否做空间 morph；false = 仅 opacity 直切（reduced-motion / 最低档）。 */
  spatial: boolean
}

/** 标准档 morph 时长；与 CSS 中的历史取值保持一致，勿随意改动观感。 */
export const PANEL_MORPH_BASE_MS = 700

/** transitionend 迟迟不来时的兜底余量（后台标签页、被打断的过渡等）。 */
export const PANEL_SETTLE_SLACK_MS = 150

export function resolvePanelMotion(input: {
  level: PanelAnimationLevel
  reduceMotion: boolean
}): PanelMotionProfile {
  // reduced-motion / 最低档：不做空间 morph，只留一次短促的 opacity 交接
  if (input.reduceMotion || input.level === 'exlight') {
    return { morphMs: 120, tabMs: 80, spatial: false }
  }
  if (input.level === 'light') {
    return { morphMs: 420, tabMs: 140, spatial: true }
  }
  return {
    morphMs: PANEL_MORPH_BASE_MS,
    tabMs: 180,
    spatial: true,
  }
}

/** 兜底 settle 超时：真实 transitionend 未到达时仍要推进相位。 */
export function settleTimeoutMs(motion: PanelMotionProfile): number {
  return motion.morphMs + PANEL_SETTLE_SLACK_MS
}
