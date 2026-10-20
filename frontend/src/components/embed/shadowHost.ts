/**
 * 嵌入页画在博客页上的那一层（伴读、创作台）：在宿主节点上开 Shadow DOM，放进自己的样式和一个容器。
 * 颜色和深浅从博客页上取（pagePaletteDom.ts），由各自的调用方接上。
 */

export interface ShadowMount {
  container: HTMLElement
  dispose: () => void
}

export function mountShadowHost(host: HTMLElement, css: string, className?: string): ShadowMount {
  const doc = host.ownerDocument
  const shadow = host.shadowRoot ?? host.attachShadow({ mode: 'open' })
  shadow.replaceChildren()
  const style = doc.createElement('style')
  style.textContent = css
  const container = doc.createElement('div')
  if (className) container.className = className
  shadow.append(style, container)
  return {
    container,
    dispose: () => shadow.replaceChildren(),
  }
}
