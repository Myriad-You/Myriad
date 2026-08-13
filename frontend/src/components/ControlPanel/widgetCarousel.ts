/**
 * 控制面板小组件区的自动翻页闸门。
 *
 * 单独抽出来的原因：这个条件有四个来源不同的输入，很容易在后续改动里漏掉
 * 其中一个，而它的真实行为依赖 `useHomeVisibility` 的定时器，
 * 在无头/隐藏标签页环境下无法运行时验证（那里定时器根本不会被创建）。
 * 纯函数化之后至少条件本身可以被锁住。
 */

export interface WidgetCarouselGate {
  /** 编辑模式下由用户自己翻页，不自动推进。 */
  isEditMode: boolean
  /** 最大页索引；0 表示只有一页，没有可轮播的内容。 */
  maxPage: number
  /**
   * 小组件当前是否真的看得见（面板已展开且停在控制页）。
   * 面板收起后组件仍然挂载，只是外壳 content-visibility: hidden。
   */
  panelVisible: boolean
  /** 指针是否停在小组件区域内 —— 视为用户正在阅读或准备点击。 */
  isHovering: boolean
}

/**
 * 是否应该继续 10 秒自动翻页。
 *
 * 注意这里只收窄条件、不放宽：任何一项不满足都停下，
 * 因此它永远不会让轮播在原本不该跑的时候跑起来。
 */
export function shouldAutoAdvanceWidgets(gate: WidgetCarouselGate): boolean {
  return (
    !gate.isEditMode &&
    gate.maxPage > 0 &&
    gate.panelVisible &&
    !gate.isHovering
  )
}
