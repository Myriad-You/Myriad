import type { Dispatch, MutableRefObject, RefObject, SetStateAction } from 'react'
import type { ShellTranslationKeys } from '../../../i18n/assembleLocale'
import type { SaveRemoteImage } from '../logic/remoteImages'
import { useCallback, useEffect, useMemo, useRef } from 'react'
import { announceDataChange } from '../../../services/dataChangeEvents'
import { saveRemoteImage, siteImageOrigins } from '../../../services/remoteMediaApi'
import {
  htmlImageSources,
  isExternalImageUrl,
  keepRemoteImages,
  markdownImageSources,
  markdownImageSrcRanges,
  rememberSaves,
  replaceMarkdownImageSrc,
} from '../logic/remoteImages'
import { showNoteNotice } from '../phantasiNotice'
import { setImageSrc } from './noteVisual'

/**
 * 粘贴进来的外站图片存进本站：粘贴照常立刻完成，存好一张换一张地址。
 * 换的时候编辑器可能已经接着打了字、协作也在同步，所以只动那张图的地址，
 * 读的是换那一刻的正文：可视层正在编辑就改 DOM 里的那几张 `<img>`，写栏改文本框里那几段，
 * 别处改状态里的原文。存不下来的保留原地址，最后说一句。
 */
export function useNoteRemoteImages(host: {
  t: ShellTranslationKeys
  format: (template: string, values: Record<string, string | number>) => string
  visualRef: RefObject<HTMLDivElement | null>
  textareaRef: RefObject<HTMLTextAreaElement | null>
  paneRef: MutableRefObject<'write' | 'visual' | 'preview'>
  visualEditing: MutableRefObject<boolean>
  commitVisualMd: (md: string) => void
  setContentMd: Dispatch<SetStateAction<string>>
  /** 测试换掉真正的接口用。 */
  save?: SaveRemoteImage
}) {
  const live = useRef(host)
  live.current = host
  const mounted = useRef(true)
  const composing = useRef(false)
  const queued = useRef<Array<[string, string]>>([])
  const save = useMemo(() => rememberSaves(host.save ?? saveRemoteImage), [host.save])

  /** 正文里还在的 `from` 图片都换成 `to`；用户已经删掉的就算了。 */
  const swapNow = useCallback((from: string, to: string) => {
    const { visualRef, textareaRef, paneRef, visualEditing, commitVisualMd, setContentMd } = live.current
    const root = visualRef.current
    if (paneRef.current === 'visual' && root && visualEditing.current) {
      // 可视层正在编辑时 DOM 才是原文：只改那几张图的属性，光标和刚打的字都不碰。
      const images = [...root.querySelectorAll('img')].filter(
        (img) => (img.dataset.src ?? img.getAttribute('src')) === from,
      )
      let md: string | null = null
      for (const img of images) md = setImageSrc(root, img, to)
      if (md != null) commitVisualMd(md)
      return
    }
    const el = textareaRef.current
    if (paneRef.current === 'write' && el) {
      // 文本框里的值就是原文。按位置换，'preserve' 让光标跟着前面的长度变化走。
      const ranges = markdownImageSrcRanges(el.value, from)
      if (ranges.length === 0) return
      for (const { start, end } of ranges.reverse()) el.setRangeText(to, start, end, 'preserve')
      setContentMd(el.value)
      return
    }
    setContentMd((current) => replaceMarkdownImageSrc(current, from, to))
  }, [])

  const flush = useCallback(() => {
    if (composing.current || !mounted.current) return
    const pending = queued.current
    queued.current = []
    for (const [from, to] of pending) swapNow(from, to)
  }, [swapNow])

  const swap = useCallback((from: string, to: string) => {
    if (!mounted.current) return
    queued.current.push([from, to])
    flush()
  }, [flush])

  // 输入法正在组字时不动正文，组完再换。
  useEffect(() => {
    mounted.current = true
    const start = () => {
      composing.current = true
    }
    const end = () => {
      composing.current = false
      // 让编辑器先处理完组字结束，再换地址。
      setTimeout(flush, 0)
    }
    document.addEventListener('compositionstart', start, true)
    document.addEventListener('compositionend', end, true)
    return () => {
      mounted.current = false
      queued.current = []
      document.removeEventListener('compositionstart', start, true)
      document.removeEventListener('compositionend', end, true)
    }
  }, [flush])

  /** 粘贴之后调：`markup` 是这次贴进去的 Markdown 或洗过的可视层 HTML。 */
  const keepPastedImages = useCallback(
    (markup: string, kind: 'markdown' | 'html') => {
      const origins = siteImageOrigins()
      const sources = (kind === 'html' ? htmlImageSources(markup) : markdownImageSources(markup))
        .filter((src) => isExternalImageUrl(src, origins))
      if (sources.length === 0) return
      void keepRemoteImages(sources, save, { onSaved: swap }).then(({ saved, kept }) => {
        if (saved.size > 0) announceDataChange('media')
        if (!mounted.current || kept.length === 0) return
        const { t, format } = live.current
        showNoteNotice(format(t.phantasi.notePasteImagesKept, { count: kept.length }), 'warning')
      })
    },
    [save, swap],
  )

  return { keepPastedImages }
}
