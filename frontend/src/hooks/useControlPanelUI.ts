import type { DynamicContentType } from '../services/DynamicContentProvider'
import type { QuoteData, WeatherData } from '../utils/dynamicContent'
import { useCallback, useReducer } from 'react'

/**
 * 动态内容类型
 */
export interface DynamicContent {
  type: DynamicContentType
  icon: string
  text: string
  subtext?: string
  /** 是否显示副文本 */
  showSubtext?: boolean
  /** 来源 Tapp ID */
  sourceTappId?: string
}

/**
 * 控制面板 UI 状态
 */
export interface ControlPanelUIState {
  // 面板展开状态
  isExpanded: boolean
  showDynamicContent: boolean
  showPanelContent: boolean
  showOverlay: boolean

  // 主题
  isDark: boolean

  // 动态内容
  dynamicContents: DynamicContent[]
  currentContentIndex: number
  isHovering: boolean
  isTransitioning: boolean

  // 信息卡片
  weatherData: WeatherData | null
  quoteData: QuoteData | null
  expandedCardIndex: number // 0=天气, 1=名言
}

/**
 * UI 动作类型
 */
type ControlPanelUIAction
  = | { type: 'TOGGLE_PANEL' }
    | { type: 'SET_EXPANDED', payload: boolean }
    | { type: 'SET_OVERLAY', payload: boolean }
    | { type: 'SET_DARK_MODE', payload: boolean }
    | { type: 'SET_DYNAMIC_CONTENTS', payload: DynamicContent[] }
    | { type: 'SET_CONTENT_INDEX', payload: number }
    | { type: 'SET_HOVERING', payload: boolean }
    | { type: 'SET_TRANSITIONING', payload: boolean }
    | { type: 'SET_WEATHER_DATA', payload: WeatherData | null }
    | { type: 'SET_QUOTE_DATA', payload: QuoteData | null }
    | { type: 'SET_EXPANDED_CARD', payload: number }
    | { type: 'START_COLLAPSE' }
    | { type: 'START_EXPAND' }

/**
 * 初始状态
 */
const initialState: ControlPanelUIState = {
  isExpanded: false,
  showDynamicContent: true,
  showPanelContent: false,
  showOverlay: false,
  isDark: false,
  dynamicContents: [],
  currentContentIndex: 0,
  isHovering: false,
  isTransitioning: false,
  weatherData: null,
  quoteData: null,
  expandedCardIndex: 0,
}

/**
 * Reducer 函数
 */
function controlPanelUIReducer(
  state: ControlPanelUIState,
  action: ControlPanelUIAction,
): ControlPanelUIState {
  switch (action.type) {
    case 'TOGGLE_PANEL':
      return { ...state, isExpanded: !state.isExpanded }

    case 'SET_EXPANDED':
      return { ...state, isExpanded: action.payload }

    case 'SET_OVERLAY':
      return { ...state, showOverlay: action.payload }

    case 'SET_DARK_MODE':
      return { ...state, isDark: action.payload }

    case 'SET_DYNAMIC_CONTENTS':
      return { ...state, dynamicContents: action.payload }

    case 'SET_CONTENT_INDEX':
      return { ...state, currentContentIndex: action.payload }

    case 'SET_HOVERING':
      return { ...state, isHovering: action.payload }

    case 'SET_TRANSITIONING':
      return { ...state, isTransitioning: action.payload }

    case 'SET_WEATHER_DATA':
      return { ...state, weatherData: action.payload }

    case 'SET_QUOTE_DATA':
      return { ...state, quoteData: action.payload }

    case 'SET_EXPANDED_CARD':
      return { ...state, expandedCardIndex: action.payload }

    case 'START_COLLAPSE':
      // 开始收缩：立即隐藏面板内容
      return {
        ...state,
        showPanelContent: false,
        showOverlay: false,
      }

    case 'START_EXPAND':
      // 开始展开：立即隐藏动态内容，显示遮罩
      return {
        ...state,
        showDynamicContent: false,
        showOverlay: true,
      }

    default:
      return state
  }
}

/**
 * 控制面板 UI 自定义 Hook
 */
export function useControlPanelUI() {
  const [state, dispatch] = useReducer(controlPanelUIReducer, initialState)

  /**
   * 切换面板展开/收缩
   */
  const togglePanel = useCallback(() => {
    if (state.isExpanded) {
      // 收缩流程
      dispatch({ type: 'START_COLLAPSE' })
      dispatch({ type: 'SET_EXPANDED', payload: false })

      // 400ms 后显示动态内容
      setTimeout(() => {
        dispatch({ type: 'SET_DYNAMIC_CONTENTS', payload: state.dynamicContents })
      }, 400)
    }
    else {
      // 展开流程
      dispatch({ type: 'START_EXPAND' })
      dispatch({ type: 'SET_EXPANDED', payload: true })

      // 400ms 后显示面板内容
      setTimeout(() => {
        dispatch({ type: 'SET_OVERLAY', payload: false })
      }, 400)
    }
  }, [state.isExpanded, state.dynamicContents])

  /**
   * 设置暗黑模式
   */
  const setDarkMode = useCallback((isDark: boolean) => {
    dispatch({ type: 'SET_DARK_MODE', payload: isDark })
    document.documentElement.classList.toggle('dark', isDark)
  }, [])

  /**
   * 设置动态内容
   */
  const setDynamicContents = useCallback((contents: DynamicContent[]) => {
    dispatch({ type: 'SET_DYNAMIC_CONTENTS', payload: contents })
  }, [])

  /**
   * 切换到下一个动态内容
   */
  const nextContent = useCallback(() => {
    dispatch({ type: 'SET_TRANSITIONING', payload: true })

    setTimeout(() => {
      const nextIndex = (state.currentContentIndex + 1) % state.dynamicContents.length
      dispatch({ type: 'SET_CONTENT_INDEX', payload: nextIndex })

      setTimeout(() => {
        dispatch({ type: 'SET_TRANSITIONING', payload: false })
      }, 50)
    }, 300)
  }, [state.currentContentIndex, state.dynamicContents.length])

  /**
   * 设置天气数据
   */
  const setWeatherData = useCallback((data: WeatherData | null) => {
    dispatch({ type: 'SET_WEATHER_DATA', payload: data })
  }, [])

  /**
   * 设置名言数据
   */
  const setQuoteData = useCallback((data: QuoteData | null) => {
    dispatch({ type: 'SET_QUOTE_DATA', payload: data })
  }, [])

  /**
   * 设置鼠标悬停状态
   */
  const setHovering = useCallback((isHovering: boolean) => {
    dispatch({ type: 'SET_HOVERING', payload: isHovering })
  }, [])

  /**
   * 切换卡片展开状态
   */
  const toggleCard = useCallback(() => {
    const nextIndex = state.expandedCardIndex === 0 ? 1 : 0
    dispatch({ type: 'SET_EXPANDED_CARD', payload: nextIndex })
  }, [state.expandedCardIndex])

  return {
    state,
    dispatch,

    // 方法
    togglePanel,
    setDarkMode,
    setDynamicContents,
    nextContent,
    setWeatherData,
    setQuoteData,
    setHovering,
    toggleCard,
  }
}
