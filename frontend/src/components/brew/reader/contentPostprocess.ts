/**
 * 正文 DOM 后处理：图片尺寸、外链、代码复制、iframe 包裹、目录、嵌入数据
 */

import type { AnnotationItem } from '../../../services/brewliaApi'
import type { TocItem } from './types'
import { useEffect } from 'react'
import { loadEmbedData } from '../../../utils/embedProcessor'

export interface UseContentPostprocessOptions {
  contentRef: React.RefObject<HTMLDivElement | null>
  baseContent: string
  showAnnotations: boolean
  annotations: AnnotationItem[]
  comments: unknown
  theme: unknown
  copyCodeLabel: string
  setToc: (toc: TocItem[]) => void
}

export function useContentPostprocess({
  contentRef,
  baseContent,
  showAnnotations,
  annotations,
  comments,
  theme,
  copyCodeLabel,
  setToc,
}: UseContentPostprocessOptions): void {
  // 处理内容中的图片和链接
  useEffect(() => {
    if (contentRef.current) {
      const images = contentRef.current.querySelectorAll('img')
      images.forEach((img) => {
        // 跳过嵌入卡片内的图片（它们有自己的样式）
        if (img.closest('.brew-embed-card, .brew-embed-exempt')) {
          return
        }

        // 跳过已处理的图片
        if (img.dataset.sizeProcessed) return
        img.dataset.sizeProcessed = 'true'

        // 添加基础样式
        img.classList.add('rounded-xl', 'h-auto', 'my-4', 'mx-auto', 'block')

        // 检测正方形图片并限制宽度
        const handleImageLoad = () => {
          const ratio = img.naturalWidth / img.naturalHeight
          const isSquare = ratio >= 0.8 && ratio <= 1.25
          const isSmall = img.naturalWidth <= 200 && img.naturalHeight <= 200

          if (isSquare || isSmall) {
            // 正方形或小图限制宽度到 35%
            img.style.maxWidth = '35%'
          }
        }

        if (img.complete && img.naturalWidth > 0) {
          handleImageLoad()
        } else {
          img.addEventListener('load', handleImageLoad, { once: true })
        }
      })

      const links = contentRef.current.querySelectorAll('a')
      links.forEach((link) => {
        link.target = '_blank'
        link.rel = 'noopener noreferrer'
      })

      // 处理代码块：添加样式和复制按钮
      const codeBlocks = contentRef.current.querySelectorAll('pre')
      codeBlocks.forEach((pre) => {
        // 跳过已处理的代码块
        if (pre.parentElement?.classList.contains('code-block-wrapper')) return

        // 创建包装容器
        const wrapper = document.createElement('div')
        wrapper.className = 'code-block-wrapper relative group my-5'

        // 将 pre 移入 wrapper
        pre.parentNode?.insertBefore(wrapper, pre)
        wrapper.appendChild(pre)

        // 添加复制按钮 - 代码块背景始终是深色的，所以按钮用浅色样式
        const copyBtn = document.createElement('button')
        copyBtn.className =
          'absolute top-3 right-3 p-1.5 rounded-lg opacity-0 group-hover:opacity-100 transition-all duration-200 bg-white/10 hover:bg-white/20 text-white/60 hover:text-white/90'
        copyBtn.innerHTML = `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"></path></svg>`
        copyBtn.title = copyCodeLabel

        copyBtn.addEventListener('click', async (e) => {
          e.preventDefault()
          e.stopPropagation()
          const code = pre.textContent || ''
          try {
            await navigator.clipboard.writeText(code)
            // 显示成功状态
            copyBtn.innerHTML = `<svg class="w-4 h-4 text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>`
            setTimeout(() => {
              copyBtn.innerHTML = `<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"></path></svg>`
            }, 2000)
          } catch (err) {
            console.error('复制失败:', err)
          }
        })

        wrapper.appendChild(copyBtn)
      })

      // 处理透传的原始 iframe（YouTube 等未被 embedProcessor 识别的嵌入）
      // 用 aspect-ratio 容器包裹，替代不稳定的 padding-bottom hack，防止滚动时闪缩
      const iframes = contentRef.current.querySelectorAll('iframe')
      iframes.forEach((iframe) => {
        // 跳过已在 brew-embed 容器内的（bilibili 等已处理的嵌入）
        if (
          iframe.closest(
            '.brew-embed-card, .brew-bilibili-embed, .rss-content-iframe-wrapper',
          )
        ) {
          return
        }
        // 跳过已处理的
        if (iframe.dataset.iframeWrapped) return
        iframe.dataset.iframeWrapped = 'true'

        const src = iframe.src || iframe.getAttribute('src') || ''
        const isMusicEmbed =
          src.includes('music.163.com') ||
          src.includes('spotify.com') ||
          src.includes('xiami.com')

        const wrapper = document.createElement('div')
        wrapper.className = `my-5 rounded-xl overflow-hidden ${isMusicEmbed ? 'aspect-3/1' : 'aspect-video'}`

        // 移除 iframe 的固定 width/height，让容器控制尺寸
        iframe.removeAttribute('width')
        iframe.removeAttribute('height')
        iframe.classList.add('w-full', 'h-full', 'border-0')
        iframe.setAttribute('loading', 'lazy')

        iframe.parentNode?.insertBefore(wrapper, iframe)
        wrapper.appendChild(iframe)
      })

      // 解析标题生成目录
      const headings = contentRef.current.querySelectorAll(
        'h1, h2, h3, h4, h5, h6',
      )
      const tocItems: TocItem[] = []

      headings.forEach((heading, index) => {
        const level = Number.parseInt(heading.tagName[1])
        const text = heading.textContent?.trim() || ''
        const id = `heading-${index}-${text.slice(0, 20).replace(/\s+/g, '-').toLowerCase()}`

        // 为标题添加 id
        heading.id = id

        if (text) {
          tocItems.push({ id, text, level })
        }
      })

      setToc(tocItems)

      // 自动加载嵌入卡片数据（网易云音乐封面、歌名等）
      // 延迟执行以确保 DOM 已完全渲染
      const loadTimer = setTimeout(() => {
        if (contentRef.current) {
          loadEmbedData(contentRef.current).catch((err) => {
            console.error('[BrewReader] 加载嵌入数据失败:', err)
          })
        }
      }, 100)

      return () => clearTimeout(loadTimer)
    }
  }, [baseContent, showAnnotations, annotations, comments, theme]) // 内容更新后重新处理 DOM
}
