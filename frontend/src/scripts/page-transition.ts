/**
 * 页面切换动画优化
 * 使用自定义加载器实现流畅自然的页面切换体验
 */

/**
 * 页面切换配置
 */
interface TransitionConfig {
  /** 最小加载显示时间（毫秒） */
  minLoadingTime?: number
  /** 是否预加载 */
  preload?: boolean
}

/**
 * 页面加载器接口定义
 */
interface PageLoader {
  show: () => void
  hide: (duration: number) => void
  isActive: () => boolean
}

/**
 * 获取页面加载器实例
 */
function getPageLoader(): PageLoader | null {
  const loader = (window as any).pageLoader
  return loader && typeof loader === 'object' ? loader : null
}

/**
 * 优化的页面导航函数
 * @param url 目标URL
 * @param config 配置选项
 */
export async function navigateWithTransition(
  url: string,
  config: TransitionConfig = {},
): Promise<void> {
  // 输入验证
  if (!url || typeof url !== 'string') {
    throw new Error('Invalid URL provided')
  }

  // URL 安全性验证
  try {
    const targetUrl = new URL(url, window.location.origin)
    // 只允许同源 URL
    if (targetUrl.origin !== window.location.origin) {
      throw new Error('Cross-origin navigation not allowed')
    }
  }
  catch (error) {
    throw new Error('Invalid URL format')
  }

  const {
    minLoadingTime = 300,
    preload = true,
  } = config

  // 验证配置参数
  const validMinLoadingTime = Math.max(0, Math.min(minLoadingTime, 5000))

  const loader = getPageLoader()

  // 如果当前已在目标页面,不执行跳转
  if (window.location.href === url) {
    return
  }

  try {
    // 1. 显示加载器
    loader?.show()

    const startTime = Date.now()

    // 2. 预加载页面(可选)
    if (preload) {
      await preloadPage(url)
    }

    // 3. 确保最小加载时间
    const elapsed = Date.now() - startTime
    if (elapsed < validMinLoadingTime) {
      await new Promise(resolve => setTimeout(resolve, validMinLoadingTime - elapsed))
    }

    // 4. 导航到新页面
    window.location.href = url
  }
  catch (error) {
    // 隐藏加载器
    loader?.hide(0)

    // 降级:直接跳转
    window.location.href = url
  }
}

/**
 * 预加载页面资源
 */
async function preloadPage(url: string): Promise<void> {
  try {
    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), 3000)

    const response = await fetch(url, {
      method: 'HEAD',
      signal: controller.signal,
    })

    clearTimeout(timeoutId)

    if (!response.ok) {
      // 静默失败,不影响用户体验
    }
  }
  catch (error) {
    // 静默失败,预加载失败不应阻止导航
  }
}

/**
 * 为所有链接添加平滑过渡
 */
export function initPageTransitions(): void {
  // 只在客户端执行
  if (typeof window === 'undefined')
    return

  // 使用事件委托优化性能
  const handleClick = (e: Event): void => {
    const target = e.target as HTMLElement
    const link = target.closest('a')

    // 检查是否是内部链接
    if (
      link
      && link.href
      && link.origin === location.origin
      && !link.hasAttribute('data-no-transition')
      && !link.hasAttribute('download')
      && !link.target
      && !link.href.includes('#') // 排除锚点链接
    ) {
      e.preventDefault()

      // 使用页面切换动画
      navigateWithTransition(link.href, {
        minLoadingTime: 300,
      }).catch(() => {
        // 如果导航失败,直接跳转
        window.location.href = link.href
      })
    }
  }

  // 拦截所有内部链接点击
  document.addEventListener('click', handleClick, { passive: false })

  // 浏览器后退/前进 - 直接跳转,不使用加载器
  const handlePopState = (): void => {
    const loader = getPageLoader()
    if (loader?.isActive()) {
      loader.hide(0)
    }
  }

  window.addEventListener('popstate', handlePopState)
}

/**
 * 添加页面进入动画
 */
export function animatePageEnter(): void {
  if (typeof window === 'undefined')
    return

  // 页面加载完成,隐藏加载器
  const loader = getPageLoader()
  if (loader?.isActive()) {
    loader.hide(300)
  }

  // 为主要内容区域添加淡入动画
  const main = document.querySelector('main')
  if (main && main instanceof HTMLElement) {
    main.style.opacity = '0'
    main.style.transform = 'translateY(10px)'

    // 使用 requestAnimationFrame 确保动画平滑
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        main.style.transition = 'opacity 0.4s ease, transform 0.4s cubic-bezier(0.4, 0, 0.2, 1)'
        main.style.opacity = '1'
        main.style.transform = 'translateY(0)'
      })
    })
  }
}

/**
 * 预加载关键页面
 * @param urls 需要预加载的URL列表
 */
export function preloadPages(urls: string[]): void {
  if (typeof document === 'undefined')
    return

  // 验证并过滤 URL
  const validUrls = urls.filter((url) => {
    try {
      const targetUrl = new URL(url, window.location.origin)
      return targetUrl.origin === window.location.origin
    }
    catch {
      return false
    }
  })

  // 使用 DocumentFragment 批量添加以提升性能
  const fragment = document.createDocumentFragment()

  validUrls.forEach((url) => {
    const link = document.createElement('link')
    link.rel = 'prefetch'
    link.href = url
    fragment.appendChild(link)
  })

  document.head.appendChild(fragment)
}

/**
 * 初始化页面切换系统
 * 应该在 DOMContentLoaded 后调用
 */
export function initTransitionSystem(): void {
  if (typeof window === 'undefined')
    return

  // 初始化链接拦截
  initPageTransitions()

  // 播放页面进入动画
  animatePageEnter()

  // 预加载常用页面
  const commonPages = ['/', '/config', '/login']
  preloadPages(commonPages)
}
