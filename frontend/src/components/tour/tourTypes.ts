export type TourAudience = 'visitor' | 'owner'
/**
 * 同路径上再切一层。缺省按浏览。
 * edit：首页编辑；canvas：资料库无限画布；persona：人物设定工作台；
 * ai-persona：AI 设置里的人设卡片。
 */
export type TourSurface = 'browse' | 'edit' | 'canvas' | 'persona' | 'ai-persona'
/** 人设生成向导等：有路径但本页不注册教程。 */
export type TourSurfacePick = TourSurface | 'none'

/** 实操门槛。未完成前不能下一步。 */
export type TourStepAction = 'open-agent'

export interface TourStepDef {
  id: string
  anchor: string
  /** 本步要用户做完才能继续。 */
  action?: TourStepAction
  /** 本步只在该动作完成后出现（实操后的介绍）。 */
  after?: TourStepAction
}

export interface TourDefinition {
  id: string
  /** Pathname after trailing-slash normalize (`/` stays `/`). */
  route: string
  /**
   * When true, also match `/route/:id` after an exact miss.
   * Exact routes always win first.
   */
  matchPrefix?: boolean
  audience: TourAudience
  surface?: TourSurface
  steps: TourStepDef[]
}
