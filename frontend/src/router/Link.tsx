/**
 * 自定义 Link 组件
 * 封装 React Router 的 Link，添加额外功能
 */

import type { LinkProps } from 'react-router-dom'
import { forwardRef } from 'react'
import { Link as RouterLink } from 'react-router-dom'

interface CustomLinkProps extends Omit<LinkProps, 'to' | 'prefetch'> {
  href: string
  prefetch?: boolean
}

/**
 * 自定义 Link 组件
 * 使用 href 属性替代 to，保持与原有代码一致
 */
export const Link = forwardRef<HTMLAnchorElement, CustomLinkProps>(
  ({ href, prefetch, ...props }, ref) => {
    // @ts-ignore - prefetch type mismatch between boolean and PrefetchBehavior
    return <RouterLink ref={ref} to={href} {...props} />
  },
)

Link.displayName = 'Link'

export default Link
