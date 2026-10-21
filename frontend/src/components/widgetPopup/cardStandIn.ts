/**
 * 收起时替卡片先出场的那一份：弹窗快落到卡片上时，框里先换成卡片的样子，
 * 落地那一刻和底下真正的卡片对齐，不留弹窗和卡片叠在一起的重影。
 *
 * 它是卡片看得见那一面的副本，挂在 body 上、叠在弹窗上面：卡片常是半透明的毛玻璃，
 * 放进弹窗里的话透出来、糊进去的都是弹窗的白底，颜色就不对了。
 * 副本只看不摸：不接事件、不进焦点、读屏不读。
 */

/** 文字相关的继承样式：副本离开了卡片原来的祖先，这些要自己带上。 */
const INHERITED = [
  'color',
  'font-family',
  'font-size',
  'font-weight',
  'font-style',
  'letter-spacing',
  'direction',
  'text-align',
  '-webkit-font-smoothing',
]

/** 和原卡片一一对应地走一遍两棵树（副本是结构相同的拷贝）。 */
function eachPair(original: Element, copy: Element, visit: (original: Element, copy: Element) => void) {
  visit(original, copy)
  const a = original.children
  const b = copy.children
  for (let index = 0; index < a.length && index < b.length; index++) eachPair(a[index], b[index], visit)
}

/**
 * 做一份卡片的副本，还没挂上去。卡片里有 iframe、视频时不做：
 * 副本里的 iframe 会重新加载一遍，视频会从头放。
 */
export function cloneCardFace(face: HTMLElement): HTMLElement | null {
  if (face.querySelector('iframe, video')) return null
  const copy = face.cloneNode(true) as HTMLElement
  eachPair(face, copy, (original, cloned) => {
    // 画布拷贝出来是空的，把当前画面画过去。
    if (original instanceof HTMLCanvasElement && cloned instanceof HTMLCanvasElement) {
      try {
        cloned.getContext('2d')?.drawImage(original, 0, 0)
      } catch {
        // WebGL 之类读不出来就空着，只有几帧。
      }
    }
    cloned.removeAttribute('id')
  })
  const host = face.parentElement ? getComputedStyle(face.parentElement) : null
  const style = copy.style
  if (host) {
    // 祖先上定义的变量（主题色之类）和文字样式，副本挂到 body 下就继承不到了。
    for (let index = 0; index < host.length; index++) {
      const name = host[index]
      if (name.startsWith('--')) style.setProperty(name, host.getPropertyValue(name))
    }
    for (const name of INHERITED) style.setProperty(name, host.getPropertyValue(name))
    // 读到的行高是换算好的像素，原样带上会让字号小的子元素也用这个高度；还原成倍数。
    const lineHeight = Number.parseFloat(host.lineHeight)
    const fontSize = Number.parseFloat(host.fontSize)
    style.lineHeight = lineHeight > 0 && fontSize > 0 ? String(lineHeight / fontSize) : 'normal'
  }
  style.position = 'fixed'
  style.left = '0'
  style.top = '0'
  style.margin = '0'
  style.width = `${face.offsetWidth}px`
  style.height = `${face.offsetHeight}px`
  style.transformOrigin = '0 0'
  style.pointerEvents = 'none'
  // 和弹窗层同一层，挂得比它晚，就在它上面。
  style.zIndex = '10020'
  style.opacity = '0'
  copy.setAttribute('aria-hidden', 'true')
  copy.inert = true
  return copy
}

/**
 * 挂上以后，把副本里的循环动画（弹幕、光晕）拨到和原卡片同一个进度，
 * 落地换回真卡片时不跳。
 */
export function syncCardAnimations(face: HTMLElement, copy: HTMLElement) {
  eachPair(face, copy, (original, cloned) => {
    const running = original.getAnimations()
    if (running.length === 0) return
    const copies = cloned.getAnimations()
    for (const animation of running) {
      const name = (animation as CSSAnimation).animationName
      const twin = name ? copies.find((other) => (other as CSSAnimation).animationName === name) : undefined
      if (twin && animation.currentTime !== null) twin.currentTime = animation.currentTime
    }
  })
}
