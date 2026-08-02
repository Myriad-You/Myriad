/**
 * 路由级 SEO：挂载时 setPageSeo，卸载时 clearPageSeo。
 * input 为 null 时只清覆盖（回到站级）。
 */

import type { PageSeoInput } from '../utils/siteMetadata'
import { useEffect } from 'react'
import {
  clearPageSeo,

  setPageSeo,
} from '../utils/siteMetadata'

/**
 * @param seo 页面 SEO；字段变化时重新应用。传 null 表示不覆盖站级。
 */
export function usePageSeo(seo: PageSeoInput | null): void {
  const title = seo?.title
  const description = seo?.description
  const image = seo?.image
  const path = seo?.path
  const noindex = seo?.noindex

  useEffect(() => {
    if (!seo) {
      clearPageSeo()
      return
    }
    setPageSeo({
      title,
      description,
      image,
      path,
      noindex,
    })
    return () => {
      clearPageSeo()
    }
    // 用展开字段做依赖，避免对象字面量每次重渲染都触发
  }, [title, description, image, path, noindex, seo === null])
}

export type { PageSeoInput }
