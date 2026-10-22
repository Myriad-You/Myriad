class PageLoader {
  private loader: HTMLElement | null
  private appRoot: HTMLElement | null
  private isShowing: boolean = true
  private hideTimeout: number | null = null
  private isAppReady: boolean = false

  constructor() {
    this.loader = document.getElementById('page-loader')
    this.appRoot = null
  }

  show(): void {
    if (!this.loader) return

    if (this.hideTimeout) {
      clearTimeout(this.hideTimeout)
      this.hideTimeout = null
    }

    this.loader.classList.remove('hidden')
    this.loader.setAttribute('aria-busy', 'true')
    this.isShowing = true
    this.isAppReady = false

    if (this.appRoot) {
      this.appRoot.classList.remove('app-ready')
    }
  }

  markAppReady(): void {
    if (this.isAppReady) return
    this.isAppReady = true

    this.appRoot = document.getElementById('app-root')

    // 后台预渲染（博客页悬停到回主站的链接上）时浏览器不出帧，requestAnimationFrame 不跑：
    // 主站早就渲染好了，加载页却要等真切过去才开始淡出，换页动效拍到的新页面就成了加载页，
    // 导航岛接不上。这时直接撤掉，不要过渡：切过去的第一帧就是主站，淡入交给换页动效。
    if ((document as Document & { prerendering?: boolean }).prerendering) {
      if (this.appRoot) {
        this.appRoot.classList.add('app-ready')
        this.appRoot.style.transition = 'none'
        this.appRoot.style.opacity = '1'
      }
      if (this.loader) {
        this.loader.style.transition = 'none'
        this.loader.classList.add('hidden')
        this.loader.setAttribute('aria-busy', 'false')
      }
      this.isShowing = false
      return
    }

    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (this.appRoot) {
          this.appRoot.classList.add('app-ready')
          /* 过渡完去掉 opacity/transition，避免 Safari 合成层导致 fixed 子元素画不出。用 setTimeout，transitionend 可能被其他属性先触发。 */
          const appRootRef = this.appRoot
          setTimeout(() => {
            if (appRootRef) {
              appRootRef.style.opacity = '1'
              appRootRef.style.transition = 'none'
            }
            // 从博客回来的换页过渡还压着博客的导航岛（utils/siteTransitionScript.ts）：主站淡入完了，
            // 底下同一个位置已经是主站的导航岛，结束过渡就看不出交接。
            ;(window as Window & { __myriadBlogReveal?: ViewTransition | null }).__myriadBlogReveal?.skipTransition()
          }, 600)
        }

        this.hide(50)
      })
    })
  }

  hide(delay: number = 200): void {
    if (!this.loader) return

    this.hideTimeout = window.setTimeout(() => {
      if (this.loader) {
        this.loader.classList.add('hidden')
        this.loader.setAttribute('aria-busy', 'false')
        this.isShowing = false
      }
    }, delay)
  }

  isActive(): boolean {
    return this.isShowing
  }

  isReady(): boolean {
    return this.isAppReady
  }
}

let pageLoader: PageLoader | undefined
const readyListeners = new Set<() => void>()

export const isDocumentReady = () => pageLoader?.isReady() ?? false

export function subscribeDocumentReady(listener: () => void): () => void {
  readyListeners.add(listener)
  return () => {
    readyListeners.delete(listener)
  }
}

export function markDocumentReady(): void {
  if (isDocumentReady()) return
  pageLoader?.markAppReady()
  for (const listener of readyListeners) listener()
}

export function initPageLoader() {
  pageLoader = new PageLoader()
}
