import React, { forwardRef } from 'react'
import { useLazyMotion } from './lazyMotion'

type SupportedTag =
  | 'div'
  | 'span'
  | 'p'
  | 'h1'
  | 'h2'
  | 'h3'
  | 'h4'
  | 'h5'
  | 'h6'
  | 'button'
  | 'a'
  | 'ul'
  | 'li'
  | 'ol'
  | 'img'
  | 'section'
  | 'article'
  | 'header'
  | 'footer'
  | 'nav'
  | 'main'
  | 'aside'
  | 'svg'
  | 'path'
  | 'g'
  | 'circle'
  | 'rect'
  | 'line'
  | 'polyline'
  | 'polygon'

// Drop motion-only props when falling back to a native tag.
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

function filterMotionProps(props: any): any {
  const filtered: any = {}
  for (const key in props) {
    if (!MOTION_PROPS.includes(key as any)) {
      filtered[key] = props[key]
    }
  }
  return filtered
}

/** Apply variants.initial / initial as CSS until motion loads (no flash). */
function getInitialStyle(props: any): React.CSSProperties | undefined {
  let initialState: any = null

  if (props.variants && props.initial && typeof props.initial === 'string') {
    initialState = props.variants[props.initial]
  }
  else if (props.initial && typeof props.initial === 'object') {
    initialState = props.initial
  }

  if (!initialState) return undefined

  const style: React.CSSProperties = {}
  const transforms: string[] = []

  if (typeof initialState.opacity === 'number') {
    style.opacity = initialState.opacity
  }

  if (typeof initialState.scale === 'number') {
    transforms.push(`scale(${initialState.scale})`)
  }
  if (typeof initialState.scaleX === 'number') {
    transforms.push(`scaleX(${initialState.scaleX})`)
  }
  if (typeof initialState.scaleY === 'number') {
    transforms.push(`scaleY(${initialState.scaleY})`)
  }

  if (initialState.y !== undefined) {
    const yVal =
      typeof initialState.y === 'number'
        ? `${initialState.y}px`
        : initialState.y
    transforms.push(`translateY(${yVal})`)
  }
  if (initialState.x !== undefined) {
    const xVal =
      typeof initialState.x === 'number'
        ? `${initialState.x}px`
        : initialState.x
    transforms.push(`translateX(${xVal})`)
  }

  if (typeof initialState.rotate === 'number') {
    transforms.push(`rotate(${initialState.rotate}deg)`)
  }
  if (typeof initialState.rotateX === 'number') {
    transforms.push(`rotateX(${initialState.rotateX}deg)`)
  }
  if (typeof initialState.rotateY === 'number') {
    transforms.push(`rotateY(${initialState.rotateY}deg)`)
  }

  if (typeof initialState.skewX === 'number') {
    transforms.push(`skewX(${initialState.skewX}deg)`)
  }
  if (typeof initialState.skewY === 'number') {
    transforms.push(`skewY(${initialState.skewY}deg)`)
  }

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

  if (
    initialState.originX !== undefined ||
    initialState.originY !== undefined
  ) {
    const ox = initialState.originX ?? 0.5
    const oy = initialState.originY ?? 0.5
    style.transformOrigin = `${ox * 100}% ${oy * 100}%`
  }

  return Object.keys(style).length > 0 ? style : undefined
}

function createShim(tag: SupportedTag) {
  const MotionShim = forwardRef<any, any>((props, ref) => {
    // Any animation-related prop needs real motion.
    const hasAnimation =
      props?.initial ||
      props?.animate ||
      props?.transition ||
      props?.variants ||
      props?.whileHover ||
      props?.whileTap ||
      props?.whileFocus ||
      props?.whileDrag ||
      props?.layout ||
      props?.layoutId ||
      props?.exit
    const { motion } = useLazyMotion(Boolean(hasAnimation))

    const { key, ...rest } = props ?? {}

    if (motion) {
      const Comp: any = motion[tag]
      return <Comp key={key} ref={ref} {...rest} />
    } else {
      // Apply initial CSS so content does not flash.
      const Tag = tag as any
      const filteredProps = filterMotionProps(rest)
      const initialStyle = getInitialStyle(props)

      if (initialStyle) {
        filteredProps.style = { ...filteredProps.style, ...initialStyle }
      }

      return <Tag key={key} ref={ref} {...filteredProps} />
    }
  })
  MotionShim.displayName = `MotionShim(${tag})`
  return MotionShim
}

export const motionShim = {
  div: createShim('div'),
  span: createShim('span'),
  p: createShim('p'),
  h1: createShim('h1'),
  h2: createShim('h2'),
  h3: createShim('h3'),
  h4: createShim('h4'),
  h5: createShim('h5'),
  h6: createShim('h6'),
  button: createShim('button'),
  a: createShim('a'),
  ul: createShim('ul'),
  ol: createShim('ol'),
  li: createShim('li'),
  img: createShim('img'),
  section: createShim('section'),
  article: createShim('article'),
  header: createShim('header'),
  footer: createShim('footer'),
  nav: createShim('nav'),
  main: createShim('main'),
  aside: createShim('aside'),
  svg: createShim('svg'),
  path: createShim('path'),
  g: createShim('g'),
  circle: createShim('circle'),
  rect: createShim('rect'),
  line: createShim('line'),
  polyline: createShim('polyline'),
  polygon: createShim('polygon'),
}

// Render children until motion loads; then real AnimatePresence.
export function AnimatePresenceShim(props: any) {
  const { AnimatePresence } = useLazyMotion(
    Boolean(
      props?.initial || props?.exit || props?.mode || props?.onExitComplete,
    ),
  )
  if (AnimatePresence) {
    const AP: any = AnimatePresence
    return <AP {...props} />
  }
  return <>{props.children}</>
}
