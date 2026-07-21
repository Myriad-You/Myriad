/**
 * 统一加载指示器 —— 全站唯一 Spinner 正典。
 *
 * 自定义权在尺寸与颜色：没有自己配色的地方一律默认全局壁纸
 * 主题色（--color-primary）；有自己配色的调用处显式声明——
 * 按钮内用 color="current" 继承按钮文字色，品牌/语义色用外层
 * text-* 类 + color="current"，或直接传任意 CSS 颜色。
 * 样式：.spinner / .spinner-center 内联于 PageLoader.astro 的
 * is:global 块（首屏即携带，水合前引导环与本组件共用）。
 * @keyframes spin 正典在 styles/animations.css（App 全局导入）；
 * PageLoader 另有首屏副本。此处不再 import CSS。
 */
import type { CSSProperties } from 'react'

export type SpinnerSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl' | number
export type SpinnerColor = 'current' | 'primary' | 'white' | (string & {})

const SIZE_PX: Record<'xs' | 'sm' | 'md' | 'lg' | 'xl', number> = {
  xs: 12,
  sm: 16,
  md: 24,
  lg: 32,
  xl: 48,
}

const COLOR_VALUE: Record<string, string> = {
  current: 'currentcolor',
  primary: 'var(--color-primary, #94a3b8)',
  white: '#fff',
}

export interface SpinnerProps {
  /** 预设 xs(12)/sm(16)/md(24)/lg(32)/xl(48)，或任意像素数 */
  size?: SpinnerSize
  /** primary(默认，全局壁纸主题色) / current(继承文字色) / white / 任意 CSS 颜色 */
  color?: SpinnerColor
  /** 环粗细 px；默认按尺寸自适应（约 size/8，夹在 1.5–4 之间） */
  thickness?: number
  /** 转一圈的秒数，默认 0.8 */
  speed?: number
  /** 延迟显现毫秒数：快于该时长完成的加载全程不闪环（CSS 实现，默认 0） */
  delay?: number
  /** 以 flex 居中容器包裹，用于区块/页面级加载位 */
  center?: boolean
  /** 无障碍标签，默认「加载中」 */
  label?: string
  className?: string
  /** @deprecated 旧 API：等价于 color="primary" / color="white" */
  variant?: 'primary' | 'white'
}

export function Spinner({
  size = 'sm',
  color,
  thickness,
  speed,
  delay,
  center = false,
  label = '加载中',
  className = '',
  variant,
}: SpinnerProps) {
  const px = typeof size === 'number' ? size : SIZE_PX[size]
  const resolved = color ?? variant ?? 'primary'
  const style = {
    '--spinner-size': `${px}px`,
    '--spinner-color': COLOR_VALUE[resolved] ?? resolved,
    '--spinner-thickness': `${
      thickness ?? Math.min(4, Math.max(1.5, Math.round(px / 8)))
    }px`,
    ...(speed ? { '--spinner-speed': `${speed}s` } : {}),
    ...(delay ? { '--spinner-delay': `${delay}ms` } : {}),
  } as CSSProperties

  const el = (
    <span
      role="status"
      aria-label={label}
      className={center ? 'spinner' : `spinner ${className}`.trim()}
      style={style}
    />
  )

  if (!center) return el
  return <div className={`spinner-center ${className}`.trim()}>{el}</div>
}

/**
 * 按钮内 Spinner —— 继承按钮文字色的超小规格
 */
export function ButtonSpinner({
  size = 'xs',
  className = '',
}: Pick<SpinnerProps, 'size' | 'className'>) {
  return <Spinner size={size} color="current" className={className} />
}

export default Spinner
