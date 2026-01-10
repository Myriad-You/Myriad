import React, { forwardRef } from 'react'
import { useLazyMotion } from './lazyMotion'

// 支持的 HTML 标签类型
type SupportedTag
  = | 'div' | 'span' | 'p' | 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6'
    | 'button' | 'a' | 'ul' | 'li' | 'ol' | 'img' | 'section' | 'article' | 'header' | 'footer' | 'nav' | 'main' | 'aside'
    | 'svg' | 'path' | 'g' | 'circle' | 'rect' | 'line' | 'polyline' | 'polygon'

// Framer Motion 特有的 props，需要在回退到原生元素时过滤掉
const MOTION_PROPS = [
  'initial',
  'animate',
  'exit',
  'transition',
  'variants',
  'whileHover',
  'whileTap',
  'whileFocus',
  'whileDrag',
  'whileInView',
  'onAnimationStart',
  'onAnimationComplete',
  'onUpdate',
  'layout',
  'layoutId',
  'layoutDependency',
  'drag',
  'dragConstraints',
  'dragElastic',
  'dragMomentum',
  'dragTransition',
  'onDragStart',
  'onDrag',
  'onDragEnd',
  'transformTemplate',
  'custom',
] as const

// 过滤掉 framer-motion 特有的 props
function filterMotionProps(props: any): any {
  const filtered: any = {}
  for (const key in props) {
    if (!MOTION_PROPS.includes(key as any)) {
      filtered[key] = props[key]
    }
  }
  return filtered
}

/**
 * 从 initial 或 variants.initial 提取初始样式
 * 用于在 motion 未加载时应用正确的初始状态，防止闪屏
 * 🔧 性能优化：扩展支持的属性列表，减少动画加载前的视觉跳变
 */
function getInitialStyle(props: any): React.CSSProperties | undefined {
  let initialState: any = null

  // 优先使用 variants 中的 initial
  if (props.variants && props.initial && typeof props.initial === 'string') {
    initialState = props.variants[props.initial]
  }
  // 直接使用 initial 对象
  else if (props.initial && typeof props.initial === 'object') {
    initialState = props.initial
  }

  if (!initialState)
    return undefined

  // 将 motion 属性转换为 CSS 样式
  const style: React.CSSProperties = {}
  const transforms: string[] = []

  // 透明度
  if (typeof initialState.opacity === 'number') {
    style.opacity = initialState.opacity
  }

  // 缩放 - 支持 scale, scaleX, scaleY
  if (typeof initialState.scale === 'number') {
    transforms.push(`scale(${initialState.scale})`)
  }
  if (typeof initialState.scaleX === 'number') {
    transforms.push(`scaleX(${initialState.scaleX})`)
  }
  if (typeof initialState.scaleY === 'number') {
    transforms.push(`scaleY(${initialState.scaleY})`)
  }

  // 平移 - 支持数字和字符串（如 '-100%'）
  if (initialState.y !== undefined) {
    const yVal = typeof initialState.y === 'number' ? `${initialState.y}px` : initialState.y
    transforms.push(`translateY(${yVal})`)
  }
  if (initialState.x !== undefined) {
    const xVal = typeof initialState.x === 'number' ? `${initialState.x}px` : initialState.x
    transforms.push(`translateX(${xVal})`)
  }

  // 🔧 新增：旋转支持
  if (typeof initialState.rotate === 'number') {
    transforms.push(`rotate(${initialState.rotate}deg)`)
  }
  if (typeof initialState.rotateX === 'number') {
    transforms.push(`rotateX(${initialState.rotateX}deg)`)
  }
  if (typeof initialState.rotateY === 'number') {
    transforms.push(`rotateY(${initialState.rotateY}deg)`)
  }

  // 🔧 新增：斜切支持
  if (typeof initialState.skewX === 'number') {
    transforms.push(`skewX(${initialState.skewX}deg)`)
  }
  if (typeof initialState.skewY === 'number') {
    transforms.push(`skewY(${initialState.skewY}deg)`)
  }

  // 🔧 新增：filter 支持
  const filters: string[] = []
  if (typeof initialState.blur === 'number' && initialState.blur > 0) {
    filters.push(`blur(${initialState.blur}px)`)
  }
  if (typeof initialState.brightness === 'number') {
    filters.push(`brightness(${initialState.brightness})`)
  }
  if (typeof initialState.contrast === 'number') {
    filters.push(`contrast(${initialState.contrast})`)
  }
  if (typeof initialState.grayscale === 'number') {
    filters.push(`grayscale(${initialState.grayscale})`)
  }
  if (typeof initialState.saturate === 'number') {
    filters.push(`saturate(${initialState.saturate})`)
  }

  if (filters.length > 0) {
    style.filter = filters.join(' ')
  }

  if (transforms.length > 0) {
    style.transform = transforms.join(' ')
  }

  // 🔧 新增：transformOrigin 支持
  if (initialState.originX !== undefined || initialState.originY !== undefined) {
    const ox = initialState.originX ?? 0.5
    const oy = initialState.originY ?? 0.5
    style.transformOrigin = `${ox * 100}% ${oy * 100}%`
  }

  return Object.keys(style).length > 0 ? style : undefined
}

// 轻量 shim：提供 motion.div / motion.span / motion.svg 等接口，但内部按需加载 framer-motion
// 使用 forwardRef 支持 ref 传递
function createShim(tag: SupportedTag) {
  const MotionShim = forwardRef<any, any>((props, ref) => {
    // 保守策略：只要传了动画相关 props 就认为需要动画
    const hasAnimation = props?.initial || props?.animate || props?.transition || props?.variants || props?.whileHover || props?.whileTap || props?.whileFocus || props?.whileDrag || props?.exit
    const { motion } = useLazyMotion(Boolean(hasAnimation))

    if (motion) {
      // motion 已加载，使用真实的 motion 组件
      const Comp: any = motion[tag]
      return <Comp ref={ref} {...props} />
    }
    else {
      // motion 未加载，使用原生元素
      // 重要：应用 initial 状态的样式，防止内容闪现
      const Tag = tag as any
      const filteredProps = filterMotionProps(props)
      const initialStyle = getInitialStyle(props)

      if (initialStyle) {
        filteredProps.style = { ...filteredProps.style, ...initialStyle }
      }

      return <Tag ref={ref} {...filteredProps} />
    }
  })
  MotionShim.displayName = `MotionShim(${tag})`
  return MotionShim
}

export const motionShim = {
  // 基础布局元素
  div: createShim('div'),
  span: createShim('span'),
  p: createShim('p'),
  // 标题元素
  h1: createShim('h1'),
  h2: createShim('h2'),
  h3: createShim('h3'),
  h4: createShim('h4'),
  h5: createShim('h5'),
  h6: createShim('h6'),
  // 交互元素
  button: createShim('button'),
  a: createShim('a'),
  // 列表元素
  ul: createShim('ul'),
  ol: createShim('ol'),
  li: createShim('li'),
  // 媒体元素
  img: createShim('img'),
  // 语义化元素
  section: createShim('section'),
  article: createShim('article'),
  header: createShim('header'),
  footer: createShim('footer'),
  nav: createShim('nav'),
  main: createShim('main'),
  aside: createShim('aside'),
  // SVG 元素
  svg: createShim('svg'),
  path: createShim('path'),
  g: createShim('g'),
  circle: createShim('circle'),
  rect: createShim('rect'),
  line: createShim('line'),
  polyline: createShim('polyline'),
  polygon: createShim('polygon'),
}

// AnimatePresence 懒加载 shim：
// 未加载 framer-motion 时直接渲染 children，加载后使用真实 AnimatePresence
export function AnimatePresenceShim(props: any) {
  const { AnimatePresence } = useLazyMotion(Boolean(props?.initial || props?.exit || props?.mode || props?.onExitComplete))
  if (AnimatePresence) {
    const AP: any = AnimatePresence
    return <AP {...props} />
  }
  return <>{props.children}</>
}
