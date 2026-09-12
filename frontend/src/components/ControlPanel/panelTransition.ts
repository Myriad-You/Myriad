// 展开/收起唯一状态：phase（issue #320）。

export type PanelAnimationLevel = 'exlight' | 'light' | 'standard'

export type PanelPhase = 'collapsed' | 'opening' | 'expanded' | 'closing'
export type PanelTab = 'control' | 'notifications'

export interface PanelState {
  phase: PanelPhase
  tab: PanelTab
  // 首次进入通知后保持挂载，tab 往返交叉淡入，不要卸载硬切。
  notifMounted: boolean
  // 相位世代号，用来作废过期 settle。
  generation: number
  // 哨兵历史是否仍未消费（系统返回先收面板）。
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
  | { type: 'open', tab?: PanelTab, historyArmed?: boolean }
  | { type: 'close' }
  | { type: 'settle', generation: number }
  | { type: 'selectTab', tab: PanelTab }

export function panelReducer(
  state: PanelState,
  action: PanelAction,
): PanelState {
  switch (action.type) {
    case 'open': {
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
      // tab 到 collapsed 才复位，避免收起首帧硬切。
      return {
        ...state,
        phase: 'closing',
        generation: state.generation + 1,
        historyArmed: false,
      }
    }

    case 'settle': {
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

export function isPanelOpen(state: PanelState): boolean {
  return state.phase === 'opening' || state.phase === 'expanded'
}

export function isPanelMorphing(state: PanelState): boolean {
  return state.phase === 'opening' || state.phase === 'closing'
}

export function showsPanelContent(state: PanelState): boolean {
  return isPanelOpen(state)
}

export function showsDynamicContent(state: PanelState): boolean {
  return state.phase === 'collapsed' || state.phase === 'closing'
}

export function showsOverlay(state: PanelState): boolean {
  return isPanelOpen(state)
}

export function showsProgressUi(state: PanelState): boolean {
  return isPanelOpen(state) && state.tab === 'control'
}

export function mountsNotifications(state: PanelState): boolean {
  return state.notifMounted
}

export interface PanelMotionProfile {
  morphMs: number
  tabMs: number
  spatial: boolean
}

export const PANEL_MORPH_BASE_MS = 700

export const PANEL_SETTLE_SLACK_MS = 150

export function resolvePanelMotion(input: {
  level: PanelAnimationLevel
  reduceMotion: boolean
}): PanelMotionProfile {
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

export function settleTimeoutMs(motion: PanelMotionProfile): number {
  return motion.morphMs + PANEL_SETTLE_SLACK_MS
}
