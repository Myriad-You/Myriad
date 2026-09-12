export const HEATMAP_WEEKS = Array.from({ length: 12 }, (_, i) => i)
export const HEATMAP_DAYS = Array.from({ length: 7 }, (_, i) => i)
export const LANES_ARRAY = Array.from({ length: 5 }, (_, i) => i)

export const DANMAKU_INITIAL = { x: '100%', opacity: 0 }
export const DANMAKU_ANIMATE = { x: '-100%', opacity: [0, 1, 1, 0] }
export function createDanmakuTransition(duration: number, delay: number) {
  return {
    repeat: 0,
    duration,
    delay,
    ease: 'linear' as const,
  }
}

export const CONTENT_FADE_INITIAL = { opacity: 0 }
export const CONTENT_FADE_ANIMATE = { opacity: 1 }
export const CONTENT_FADE_EXIT = { opacity: 0 }
export const CONTENT_FADE_TRANSITION = { duration: 0.5 }

export const CONTENT_SLIDE_INITIAL = { opacity: 0, y: 10 }
export const CONTENT_SLIDE_ANIMATE = { opacity: 1, y: 0 }
export const CONTENT_SLIDE_EXIT = { opacity: 0, y: -10 }
export const CONTENT_SLIDE_TRANSITION = { duration: 0.5 }
