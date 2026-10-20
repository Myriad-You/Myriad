/**
 * 嵌入页画在博客页上的那一层（伴读、创作台）：在宿主节点上开 Shadow DOM，放进自己的样式和一个容器；
 * 明暗跟着嵌入页自己的 `<html>` 上的 dark 类，写到宿主节点的 `data-scheme`——
 * 嵌入页和博客页读的是同一个主题偏好，切深浅色时两边一起换。
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
  const html = document.documentElement
  const syncScheme = () => {
    host.dataset.scheme = html.classList.contains('dark') ? 'dark' : 'light'
  }
  syncScheme()
  const observer = new MutationObserver(syncScheme)
  observer.observe(html, { attributes: true, attributeFilter: ['class'] })
  return {
    container,
    dispose: () => {
      observer.disconnect()
      shadow.replaceChildren()
    },
  }
}
