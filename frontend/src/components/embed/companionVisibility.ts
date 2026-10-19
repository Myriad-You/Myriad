/**
 * 两侧胶囊什么时候露出来。照旧阅读器的意思，但只认读者真想用它的信号：
 * - 往下读（累计 24px）就收起，往上翻（累计 24px）就出来；回到页面顶端一直在。
 *   按累计算：触控板和平滑滚动一次只挪一两像素。
 * - 鼠标或手指伸到正文两侧（胶囊在的地方）就出来；在页面别处动鼠标不算，
 *   不然读着读着胶囊老冒出来。
 * - 出来后停 4 秒又收起；离开胶囊 2 秒收起。鼠标停在上面、焦点在里面、面板开着时一直在。
 * - 自己滚动页面（回到顶部、去评论、跳到注释）的那一阵不算读者在滚。
 */

export const SCROLL_INTENT = 24
export const TOP_ZONE = 80
export const IDLE_MS = 4000
export const AFTER_LEAVE_MS = 2000
/** 自己滚动页面后这么久内的滚动不算数；平滑滚动大约要这么久。 */
export const OWN_SCROLL_MS = 900

export class ChromeVisibility {
  visible = true
  private held = false
  /** 这一段同方向滚动是从哪开始的。 */
  private anchor: number
  private lastY: number
  private direction: 1 | -1 = 1
  private timer: ReturnType<typeof setTimeout> | undefined
  private ownUntil = 0

  constructor(
    scrollY: number,
    private readonly onChange: (visible: boolean) => void,
    private readonly now: () => number = Date.now,
  ) {
    this.anchor = scrollY
    this.lastY = scrollY
    this.arm(IDLE_MS)
  }

  scroll(y: number): void {
    const last = this.lastY
    this.lastY = y
    if (this.now() < this.ownUntil) {
      this.anchor = y
      return
    }
    if (y <= TOP_ZONE) {
      this.anchor = y
      this.show(IDLE_MS)
      return
    }
    if (y === last) return
    const direction = y > last ? 1 : -1
    if (direction !== this.direction) {
      this.direction = direction
      this.anchor = last
    }
    const moved = y - this.anchor
    if (moved >= SCROLL_INTENT) {
      this.anchor = y
      this.hide()
    } else if (moved <= -SCROLL_INTENT) {
      this.anchor = y
      this.show(AFTER_LEAVE_MS)
    }
  }

  /** 伸手去拿：鼠标或手指到了胶囊那一带。 */
  reach(): void {
    this.show(IDLE_MS)
  }

  hold(on: boolean): void {
    if (on === this.held) return
    this.held = on
    if (on) {
      clearTimeout(this.timer)
      this.set(true)
    } else {
      this.arm(AFTER_LEAVE_MS)
    }
  }

  /** 接下来一阵的滚动是自己滚的。 */
  ownScroll(): void {
    this.ownUntil = this.now() + OWN_SCROLL_MS
  }

  dispose(): void {
    clearTimeout(this.timer)
  }

  private show(idle: number): void {
    this.set(true)
    this.arm(idle)
  }

  private hide(): void {
    if (this.held) return
    clearTimeout(this.timer)
    this.set(false)
  }

  private arm(ms: number): void {
    clearTimeout(this.timer)
    if (this.held) return
    this.timer = setTimeout(() => this.hide(), ms)
  }

  private set(visible: boolean): void {
    if (visible === this.visible) return
    this.visible = visible
    this.onChange(visible)
  }
}
