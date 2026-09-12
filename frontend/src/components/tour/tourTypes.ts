export type TourAudience = 'visitor' | 'owner'
export type TourSurface = 'browse' | 'edit' | 'canvas' | 'persona' | 'ai-persona'
export type TourSurfacePick = TourSurface | 'none'

// 未完成前不能下一步。
export type TourStepAction = 'open-agent'

export interface TourStepDef {
  id: string
  anchor: string
  action?: TourStepAction
  after?: TourStepAction
}

export interface TourDefinition {
  id: string
  route: string
  matchPrefix?: boolean
  audience: TourAudience
  surface?: TourSurface
  steps: TourStepDef[]
}
