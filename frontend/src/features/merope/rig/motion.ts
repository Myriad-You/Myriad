export type GazeSource = 'pointer' | 'camera' | 'performance'

export interface GazeTarget {
  x: number
  y: number
  attention?: number
  source?: GazeSource
}

