/**
 * Tapp Playground 专用图标（stroke = currentColor）
 * 主图标使用 lucide 的锥形烧瓶：Playground = 实验场
 */
import { FlaskConical } from 'lucide-react'

interface IconProps {
  className?: string
  style?: React.CSSProperties
}

/** 锥形烧瓶：「Tapp 实验场」 */
export function TappPlaygroundIcon({ className, style }: IconProps) {
  return <FlaskConical className={className} style={style} aria-hidden="true" />
}

/** 时间线列表：「生成过程 / Agent 工作轨迹」 */
export function PlaygroundTraceIcon({ className, style }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={style}
      aria-hidden="true"
    >
      <circle cx="5.5" cy="5" r="1.75" />
      <circle cx="5.5" cy="12" r="1.75" />
      <circle cx="5.5" cy="19" r="1.75" />
      <path d="M5.5 6.75v3.5M5.5 13.75v3.5" />
      <path d="M10.5 5H20M10.5 12H20M10.5 19H20" />
    </svg>
  )
}
