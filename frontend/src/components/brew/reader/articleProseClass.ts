/**
 * 阅读器正文 prose 类名（浅色 / 暗色两套）
 */

export function getArticleProseClass(
  isDark: boolean,
  themeTextClass: string,
): string {
  return `
              prose prose-lg max-w-none min-w-0 ${themeTextClass}
              /* 超长 URL / 无空格串不得撑破阅读器 */
              break-words [overflow-wrap:anywhere]

              /* 标题 - 简洁无装饰 */
              prose-headings:font-semibold prose-headings:leading-snug
              prose-h1:text-[1.5em] prose-h1:mt-8 prose-h1:mb-4
              prose-h2:text-[1.25em] prose-h2:mt-7 prose-h2:mb-3
              prose-h3:text-[1.1em] prose-h3:mt-6 prose-h3:mb-2
              prose-h4:text-[1em] prose-h4:mt-5 prose-h4:mb-2 prose-h4:font-medium

              /* 段落 */
              prose-p:my-[1em]

              /* 加粗文本 - 明确样式防止被覆盖 */
              prose-strong:font-bold prose-strong:no-underline
              [&_strong]:font-bold [&_strong]:no-underline [&_strong]:not-italic
              [&_b]:font-bold [&_b]:no-underline [&_b]:not-italic

              /* 删除线 - 仅对 del/s/strike 应用 */
              [&_del]:line-through [&_del]:opacity-60
              [&_s]:line-through [&_s]:opacity-60
              [&_strike]:line-through [&_strike]:opacity-60

              /* 链接 - 简洁下划线 + 防溢出 */
              prose-a:font-normal prose-a:underline prose-a:underline-offset-2
              prose-a:decoration-1 prose-a:transition-colors
              prose-a:wrap-break-word [&_a]:overflow-wrap-anywhere

              /* 列表 - 紧凑 */
              prose-ul:my-4 prose-ul:pl-5
              prose-ol:my-4 prose-ol:pl-5
              prose-li:my-1 prose-li:pl-0.5

              /* 引用块 - 轻盈圆角 */
              prose-blockquote:not-italic prose-blockquote:font-normal
              prose-blockquote:border-0 prose-blockquote:rounded-2xl
              prose-blockquote:px-5 prose-blockquote:py-4 prose-blockquote:my-5

              /* 行内代码 - 柔和 */
              prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded-lg
              prose-code:text-[0.9em] prose-code:font-normal
              prose-code:before:content-none prose-code:after:content-none

              /* 代码块 - 干净圆角 + 相对定位（支持复制按钮） */
              prose-pre:rounded-2xl prose-pre:px-5 prose-pre:py-4
              prose-pre:overflow-x-auto prose-pre:text-[0.875em]
              prose-pre:leading-relaxed prose-pre:relative
              /* 代码块内的code不要额外样式 */
              [&_pre_code]:p-0 [&_pre_code]:bg-transparent [&_pre_code]:rounded-none
              [&_pre_code]:text-inherit
              /* 代码块容器（有复制按钮时）*/
              [&_.code-block-wrapper]:my-5

              /* 图片 - 自然圆角 */
              prose-img:rounded-2xl prose-img:mx-auto prose-img:my-5

              /* 分隔线 - 极简 */
              prose-hr:my-8 prose-hr:border-0 prose-hr:h-px

              /* 表格 - 简约 */
              prose-table:my-5 prose-table:w-full prose-table:text-[0.9em]
              prose-thead:border-0
              prose-th:py-2.5 prose-th:px-3 prose-th:text-left prose-th:font-medium
              prose-td:py-2 prose-td:px-3
              [&_table]:rounded-xl [&_table]:overflow-hidden

              /* KaTeX 数学公式 */
              [&_.katex]:text-[1.05em]
              [&_.katex-display]:my-5 [&_.katex-display]:py-4 [&_.katex-display]:px-4
              [&_.katex-display]:overflow-x-auto [&_.katex-display]:rounded-2xl

              /* figure */
              prose-figure:my-6
              prose-figcaption:text-center prose-figcaption:text-[0.85em] prose-figcaption:mt-2
              prose-figcaption:opacity-60

              /* details 折叠 */
              [&_details]:my-4 [&_details]:rounded-2xl [&_details]:overflow-hidden
              [&_summary]:cursor-pointer [&_summary]:py-3 [&_summary]:px-4
              [&_summary]:font-medium [&_summary]:select-none
              [&_details[open]_summary]:mb-2

              /* kbd 按键 */
              [&_kbd]:px-1.5 [&_kbd]:py-0.5 [&_kbd]:rounded-lg
              [&_kbd]:text-[0.8em] [&_kbd]:font-mono

              /* mark 高亮 */
              [&_mark]:px-1 [&_mark]:rounded-md [&_mark]:bg-transparent

              /* 脚注 */
              prose-footnotes:text-[0.85em] prose-footnotes:mt-8 prose-footnotes:opacity-70

              /* 嵌入卡片通用样式 */
              [&_.brew-embed-card]:my-6 [&_.brew-embed-card]:font-sans
              [&_.brew-embed-card]:text-base [&_.brew-embed-card]:leading-normal
              [&_.brew-embed-card_*]:no-underline

              /* RSS 内容适配样式 */

              /* RSS 图片 - 响应式 + 圆角 */
              [&_.rss-content-image]:rounded-xl [&_.rss-content-image]:max-w-full
              [&_.rss-content-image]:h-auto [&_.rss-content-image]:mx-auto
              [&_.rss-content-image]:block [&_.rss-content-image]:my-5

              /* RSS 图片容器 figure */
              [&_.rss-content-figure]:my-6 [&_.rss-content-figure]:text-center
              [&_.rss-content-figcaption]:text-[0.85em] [&_.rss-content-figcaption]:mt-2
              [&_.rss-content-figcaption]:opacity-60

              /* RSS 视频 */
              [&_.rss-content-video]:rounded-xl [&_.rss-content-video]:w-full
              [&_.rss-content-video]:my-5

              /* RSS 音频 */
              [&_.rss-content-audio]:w-full [&_.rss-content-audio]:my-4

              /* RSS iframe 包装 - 响应式容器（aspect-ratio 替代 padding-bottom hack）*/
              [&_.rss-content-iframe-wrapper]:w-full
              [&_.rss-content-iframe-wrapper]:my-5 [&_.rss-content-iframe-wrapper]:rounded-xl
              [&_.rss-content-iframe-wrapper]:overflow-hidden
              [&_.rss-content-iframe-wrapper.aspect-video]:aspect-video
              [&_.rss-content-iframe-wrapper.aspect-wide]:aspect-3/1
              [&_.rss-content-iframe-wrapper_iframe]:w-full [&_.rss-content-iframe-wrapper_iframe]:h-full
              [&_.rss-content-iframe-wrapper_iframe]:border-0

              /* RSS 表格包装 - 横向滚动 */
              [&_.rss-content-table-wrapper]:overflow-x-auto [&_.rss-content-table-wrapper]:my-5
              [&_.rss-content-table-wrapper]:rounded-xl
              [&_.rss-content-table]:w-full [&_.rss-content-table]:text-[0.9em]
              [&_.rss-content-table]:border-collapse
              [&_.rss-content-th]:py-2 [&_.rss-content-th]:px-3
              [&_.rss-content-th]:text-left [&_.rss-content-th]:font-medium
              [&_.rss-content-td]:py-2 [&_.rss-content-td]:px-3

              /* RSS 描述列表 */
              [&_.rss-content-dl]:my-4
              [&_.rss-content-dt]:font-semibold [&_.rss-content-dt]:mt-3
              [&_.rss-content-dd]:ml-4 [&_.rss-content-dd]:pl-4 [&_.rss-content-dd]:mt-1

              /* RSS 折叠组件 */
              [&_.rss-content-details]:my-4 [&_.rss-content-details]:rounded-xl
              [&_.rss-content-details]:overflow-hidden
              [&_.rss-content-summary]:cursor-pointer [&_.rss-content-summary]:py-3
              [&_.rss-content-summary]:px-4 [&_.rss-content-summary]:font-medium
              [&_.rss-content-summary]:select-none [&_.rss-content-summary]:transition-colors
              [&_.rss-content-details[open]_.rss-content-summary]:border-b

              /* RSS 链接 - 文字断行 */
              [&_.rss-content-link]:wrap-break-word [&_.rss-content-link]:underline
              [&_.rss-content-link]:underline-offset-2 [&_.rss-content-link]:decoration-1

              /* RSS kbd 按键样式 */
              [&_.rss-content-kbd]:px-1.5 [&_.rss-content-kbd]:py-0.5
              [&_.rss-content-kbd]:rounded [&_.rss-content-kbd]:text-[0.85em]
              [&_.rss-content-kbd]:font-mono [&_.rss-content-kbd]:border

              /* RSS mark 高亮 */
              [&_.rss-content-mark]:px-0.5 [&_.rss-content-mark]:rounded

              /* RSS abbr 缩写 */
              [&_.rss-content-abbr]:border-b [&_.rss-content-abbr]:border-dashed
              [&_.rss-content-abbr]:cursor-help

              /* RSS 分隔线 */
              [&_.rss-content-hr]:border-0 [&_.rss-content-hr]:h-px [&_.rss-content-hr]:my-8

              /* RSS 删除线/插入 */
              [&_.rss-content-del]:line-through [&_.rss-content-del]:opacity-60
              [&_.rss-content-ins]:underline

              /* RSS 小号文本 */
              [&_.rss-content-small]:text-[0.85em] [&_.rss-content-small]:opacity-80

              /* RSS 上下标 */
              [&_.rss-content-sup]:text-[0.75em]
              [&_.rss-content-sub]:text-[0.75em]

              /* RSS 时间戳 */
              [&_.rss-content-time]:tabular-nums

              /* RSS 类别标签 */
              [&_.rss-content-category]:inline-block [&_.rss-content-category]:px-2
              [&_.rss-content-category]:py-0.5 [&_.rss-content-category]:text-xs
              [&_.rss-content-category]:rounded-full [&_.rss-content-category]:mr-1

              /* RSS 语义标签 */
              [&_.rss-content-aside]:my-4 [&_.rss-content-aside]:p-4
              [&_.rss-content-aside]:rounded-xl [&_.rss-content-aside]:opacity-80
              [&_.rss-content-header]:mb-4
              [&_.rss-content-footer]:mt-4 [&_.rss-content-footer]:text-sm
              [&_.rss-content-footer]:opacity-70

              /* 网络搜索摘要样式 - 浅色主题 */
              /* 摘要容器 - 不设置固定字体大小，继承阅读器设置 */
              [&_.web-search-summary]:leading-[inherit]

              /* 摘要段落 - 继承阅读器的行高和字体 */
              [&_.web-search-summary_p]:mb-4 [&_.web-search-summary_p]:last:mb-0

              /* 页脚区域 */
              [&_.web-search-footer]:mt-8 [&_.web-search-footer]:pt-6
              [&_.web-search-footer]:border-t [&_.web-search-footer]:border-black/10
              [&_.web-search-footer]:flex [&_.web-search-footer]:items-center
              [&_.web-search-footer]:justify-between [&_.web-search-footer]:gap-4

              /* AI 生成说明 */
              [&_.web-search-note]:text-sm [&_.web-search-note]:opacity-50
              [&_.web-search-note]:m-0

              /* 原文链接按钮 */
              [&_.web-search-link]:inline-flex [&_.web-search-link]:items-center
              [&_.web-search-link]:gap-1.5 [&_.web-search-link]:text-sm
              [&_.web-search-link]:px-3 [&_.web-search-link]:py-1.5
              [&_.web-search-link]:rounded-lg [&_.web-search-link]:no-underline
              [&_.web-search-link]:bg-black/5 [&_.web-search-link]:hover:bg-black/10
              [&_.web-search-link]:transition-colors

              ${
                isDark
                  ? `
                /* 暗色主题 */
                prose-invert

                /* 链接 */
                prose-a:text-blue-400 prose-a:decoration-blue-400/40
                hover:prose-a:text-blue-300 hover:prose-a:decoration-blue-300/60

                /* 引用块 */
                prose-blockquote:bg-white/3

                /* 行内代码 */
                prose-code:bg-white/8 prose-code:text-amber-200/90

                /* 代码块 */
                prose-pre:bg-white/4

                /* 分隔线 */
                prose-hr:bg-white/6

                /* 表格 */
                [&_table]:bg-white/2
                [&_thead]:bg-white/3
                [&_tbody_tr:nth-child(even)]:bg-white/2

                /* 数学公式 */
                [&_.katex-display]:bg-white/3

                /* details */
                [&_details]:bg-white/3
                [&_summary:hover]:bg-white/5

                /* kbd */
                [&_kbd]:bg-white/8

                /* mark */
                [&_mark]:text-amber-200 [&_mark]:bg-amber-500/20

                /* RSS 内容样式 - 暗色主题 */

                /* RSS 引用块 */
                [&_.rss-content-blockquote]:bg-white/3 [&_.rss-content-blockquote]:border-white/10

                /* RSS 代码 */
                [&_.rss-content-pre]:bg-white/4
                [&_.rss-content-inline-code]:bg-white/8

                /* RSS 表格 */
                [&_.rss-content-table]:border-white/10
                [&_.rss-content-thead]:bg-white/5
                [&_.rss-content-th]:border-white/10
                [&_.rss-content-td]:border-white/10
                [&_.rss-content-tr:nth-child(even)]:bg-white/2

                /* RSS 描述列表 */
                [&_.rss-content-dd]:border-white/10

                /* RSS 折叠 */
                [&_.rss-content-details]:bg-white/3
                [&_.rss-content-summary]:hover:bg-white/5
                [&_.rss-content-details[open]_.rss-content-summary]:border-white/10

                /* RSS kbd */
                [&_.rss-content-kbd]:bg-white/8 [&_.rss-content-kbd]:border-white/20

                /* RSS mark */
                [&_.rss-content-mark]:bg-yellow-500/30

                /* RSS abbr */
                [&_.rss-content-abbr]:border-white/30

                /* RSS 分隔线 */
                [&_.rss-content-hr]:bg-white/10

                /* RSS 类别标签 */
                [&_.rss-content-category]:bg-white/10

                /* RSS 侧边栏 */
                [&_.rss-content-aside]:bg-white/3

                /* Brewlia 注释样式 - 暗色主题 */
                [&_.brewlia-annotation]:cursor-help [&_.brewlia-annotation]:rounded [&_.brewlia-annotation]:px-0.5
                [&_.brewlia-annotation]:transition-all [&_.brewlia-annotation]:duration-200
                [&_.brewlia-annotation]:border-b-2 [&_.brewlia-annotation]:border-dotted
                [&_.brewlia-annotation]:break-words [&_.brewlia-annotation]:[overflow-wrap:anywhere]
                [&_.brewlia-annotation[data-type="term"]]:text-orange-300 [&_.brewlia-annotation[data-type="term"]]:bg-orange-500/15 [&_.brewlia-annotation[data-type="term"]]:border-orange-400/50
                [&_.brewlia-annotation[data-type="reference"]]:text-blue-300 [&_.brewlia-annotation[data-type="reference"]]:bg-blue-500/15 [&_.brewlia-annotation[data-type="reference"]]:border-blue-400/50
                [&_.brewlia-annotation[data-type="implicit"]]:text-purple-300 [&_.brewlia-annotation[data-type="implicit"]]:bg-purple-500/15 [&_.brewlia-annotation[data-type="implicit"]]:border-purple-400/50
                [&_.brewlia-annotation[data-type="context"]]:text-green-300 [&_.brewlia-annotation[data-type="context"]]:bg-green-500/15 [&_.brewlia-annotation[data-type="context"]]:border-green-400/50
                [&_.brewlia-annotation[data-type="abbreviation"]]:text-pink-300 [&_.brewlia-annotation[data-type="abbreviation"]]:bg-pink-500/15 [&_.brewlia-annotation[data-type="abbreviation"]]:border-pink-400/50
                [&_.brewlia-annotation:hover]:ring-2 [&_.brewlia-annotation:hover]:ring-current/30
                [&_.brewlia-highlight-flash]:animate-pulse [&_.brewlia-highlight-flash]:ring-2 [&_.brewlia-highlight-flash]:ring-purple-400

                /* 网络搜索摘要样式 - 暗色主题 */
                /* 页脚区域 - 暗色主题 */
                [&_.web-search-footer]:border-white/10

                /* 原文链接按钮 - 暗色主题 */
                [&_.web-search-link]:bg-white/10 [&_.web-search-link]:hover:bg-white/15

                /* Notion 内容样式 - 暗色主题 */

                /* Notion 颜色 - 文字 */
                [&_.notion-gray]:text-gray-400
                [&_.notion-brown]:text-amber-400
                [&_.notion-orange]:text-orange-400
                [&_.notion-yellow]:text-yellow-400
                [&_.notion-green]:text-green-400
                [&_.notion-blue]:text-blue-400
                [&_.notion-purple]:text-purple-400
                [&_.notion-pink]:text-pink-400
                [&_.notion-red]:text-red-400

                /* Notion 颜色 - 背景 */
                [&_.notion-bg-gray]:bg-gray-500/20 [&_.notion-bg-gray]:px-1 [&_.notion-bg-gray]:rounded
                [&_.notion-bg-brown]:bg-amber-500/20 [&_.notion-bg-brown]:px-1 [&_.notion-bg-brown]:rounded
                [&_.notion-bg-orange]:bg-orange-500/20 [&_.notion-bg-orange]:px-1 [&_.notion-bg-orange]:rounded
                [&_.notion-bg-yellow]:bg-yellow-500/20 [&_.notion-bg-yellow]:px-1 [&_.notion-bg-yellow]:rounded
                [&_.notion-bg-green]:bg-green-500/20 [&_.notion-bg-green]:px-1 [&_.notion-bg-green]:rounded
                [&_.notion-bg-blue]:bg-blue-500/20 [&_.notion-bg-blue]:px-1 [&_.notion-bg-blue]:rounded
                [&_.notion-bg-purple]:bg-purple-500/20 [&_.notion-bg-purple]:px-1 [&_.notion-bg-purple]:rounded
                [&_.notion-bg-pink]:bg-pink-500/20 [&_.notion-bg-pink]:px-1 [&_.notion-bg-pink]:rounded
                [&_.notion-bg-red]:bg-red-500/20 [&_.notion-bg-red]:px-1 [&_.notion-bg-red]:rounded

                /* Notion Callout */
                [&_.notion-callout]:flex [&_.notion-callout]:items-start [&_.notion-callout]:gap-3
                [&_.notion-callout]:p-4 [&_.notion-callout]:my-4 [&_.notion-callout]:rounded-xl
                [&_.notion-callout]:bg-white/4 [&_.notion-callout]:border [&_.notion-callout]:border-white/10
                [&_.notion-callout-icon]:text-xl [&_.notion-callout-icon]:shrink-0
                [&_.notion-callout-icon]:w-6 [&_.notion-callout-icon]:h-6 [&_.notion-callout-icon]:object-contain
                [&_.notion-callout-content]:flex-1 [&_.notion-callout-content]:min-w-0

                /* Notion Quote */
                [&_.notion-quote]:pl-4 [&_.notion-quote]:py-1 [&_.notion-quote]:my-4
                [&_.notion-quote]:bg-white/4 [&_.notion-quote]:rounded-xl

                /* Notion Todo */
                [&_.notion-todo]:flex [&_.notion-todo]:items-start [&_.notion-todo]:gap-2 [&_.notion-todo]:my-1
                [&_.notion-checkbox]:w-5 [&_.notion-checkbox]:h-5 [&_.notion-checkbox]:shrink-0
                [&_.notion-checkbox]:border-2 [&_.notion-checkbox]:border-white/30 [&_.notion-checkbox]:rounded
                [&_.notion-checkbox.checked]:bg-blue-500 [&_.notion-checkbox.checked]:border-blue-500
                [&_.notion-checkbox.checked]:after:content-['✓'] [&_.notion-checkbox.checked]:after:text-white
                [&_.notion-checkbox.checked]:after:text-xs [&_.notion-checkbox.checked]:after:flex
                [&_.notion-checkbox.checked]:after:items-center [&_.notion-checkbox.checked]:after:justify-center
                [&_.notion-todo-text.checked]:line-through [&_.notion-todo-text.checked]:opacity-60

                /* Notion Toggle */
                [&_.notion-toggle]:bg-white/3 [&_.notion-toggle]:border [&_.notion-toggle]:border-white/10
                [&_.notion-toggle]:rounded-xl [&_.notion-toggle]:my-3
                [&_.notion-toggle_summary]:px-4 [&_.notion-toggle_summary]:py-3

                /* Notion Image */
                [&_.notion-image]:my-6
                [&_.notion-image_img]:rounded-xl [&_.notion-image_img]:w-full
                [&_.notion-image_figcaption]:text-center [&_.notion-image_figcaption]:text-sm
                [&_.notion-image_figcaption]:mt-2 [&_.notion-image_figcaption]:opacity-60

                /* Notion Video */
                [&_.notion-video]:my-6
                [&_.notion-video-embed]:aspect-video [&_.notion-video-embed]:w-full
                [&_.notion-video-embed]:rounded-xl [&_.notion-video-embed]:overflow-hidden
                [&_.notion-video-embed_iframe]:w-full [&_.notion-video-embed_iframe]:h-full
                [&_.notion-video_video]:w-full [&_.notion-video_video]:rounded-xl

                /* Notion Audio */
                [&_.notion-audio]:my-4
                [&_.notion-audio_audio]:w-full

                /* Notion Bookmark */
                [&_.notion-bookmark]:flex [&_.notion-bookmark]:items-center [&_.notion-bookmark]:gap-3
                [&_.notion-bookmark]:p-4 [&_.notion-bookmark]:my-4 [&_.notion-bookmark]:rounded-xl
                [&_.notion-bookmark]:bg-white/4 [&_.notion-bookmark]:border [&_.notion-bookmark]:border-white/10
                [&_.notion-bookmark]:no-underline [&_.notion-bookmark]:hover:bg-white/6
                [&_.notion-bookmark-icon]:text-lg
                [&_.notion-bookmark-title]:font-medium [&_.notion-bookmark-title]:flex-1
                [&_.notion-bookmark-url]:text-sm [&_.notion-bookmark-url]:opacity-50 [&_.notion-bookmark-url]:truncate [&_.notion-bookmark-url]:max-w-48

                /* Notion Link Preview */
                [&_.notion-link-preview]:inline-flex [&_.notion-link-preview]:items-center [&_.notion-link-preview]:gap-1.5
                [&_.notion-link-preview]:px-2 [&_.notion-link-preview]:py-0.5 [&_.notion-link-preview]:rounded-md
                [&_.notion-link-preview]:bg-white/6 [&_.notion-link-preview]:no-underline
                [&_.notion-link-preview]:hover:bg-white/10

                /* Notion File */
                [&_.notion-file]:inline-flex [&_.notion-file]:items-center [&_.notion-file]:gap-2
                [&_.notion-file]:px-3 [&_.notion-file]:py-2 [&_.notion-file]:my-2 [&_.notion-file]:rounded-lg
                [&_.notion-file]:bg-white/4 [&_.notion-file]:border [&_.notion-file]:border-white/10
                [&_.notion-file]:no-underline [&_.notion-file]:hover:bg-white/8

                /* Notion Embed */
                [&_.notion-embed]:my-6
                [&_.notion-embed-wrapper]:aspect-video [&_.notion-embed-wrapper]:w-full
                [&_.notion-embed-wrapper]:rounded-xl [&_.notion-embed-wrapper]:overflow-hidden
                [&_.notion-embed-wrapper_iframe]:w-full [&_.notion-embed-wrapper_iframe]:h-full

                /* Notion PDF */
                [&_.notion-pdf]:my-6
                [&_.notion-pdf-embed]:w-full [&_.notion-pdf-embed]:h-150 [&_.notion-pdf-embed]:rounded-xl
                [&_.notion-pdf-embed]:border [&_.notion-pdf-embed]:border-white/10

                /* Notion Equation */
                [&_.notion-equation]:my-4 [&_.notion-equation]:py-4 [&_.notion-equation]:px-6
                [&_.notion-equation]:bg-white/3 [&_.notion-equation]:rounded-xl
                [&_.notion-equation]:overflow-x-auto [&_.notion-equation]:text-center

                /* Notion Table */
                [&_.notion-table]:w-full [&_.notion-table]:my-4 [&_.notion-table]:border-collapse
                [&_.notion-table]:rounded-xl [&_.notion-table]:overflow-hidden
                [&_.notion-table_td]:px-3 [&_.notion-table_td]:py-2
                [&_.notion-table_td]:border [&_.notion-table_td]:border-white/10
                [&_.notion-table.has-header_tr:first-child]:bg-white/5
                [&_.notion-table.has-header_tr:first-child_td]:font-medium

                /* Notion Columns */
                [&_.notion-columns]:flex [&_.notion-columns]:gap-4 [&_.notion-columns]:my-4
                [&_.notion-column]:flex-1 [&_.notion-column]:min-w-0

                /* Notion Page Link */
                [&_.notion-page-link]:inline-flex [&_.notion-page-link]:items-center [&_.notion-page-link]:gap-1.5
                [&_.notion-page-link]:px-2 [&_.notion-page-link]:py-1 [&_.notion-page-link]:rounded-md
                [&_.notion-page-link]:bg-white/4 [&_.notion-page-link]:no-underline
                [&_.notion-page-link]:hover:bg-white/8

                /* Notion Code */
                [&_.notion-code]:my-4
                [&_.notion-code_pre]:rounded-xl [&_.notion-code_pre]:overflow-x-auto
                [&_.notion-code_figcaption]:text-center [&_.notion-code_figcaption]:text-sm
                [&_.notion-code_figcaption]:mt-2 [&_.notion-code_figcaption]:opacity-50

              `
                  : `
                /* 浅色主题 */

                /* 链接 */
                prose-a:text-amber-700 prose-a:decoration-amber-600/30
                hover:prose-a:text-amber-800 hover:prose-a:decoration-amber-700/50

                /* 引用块 */
                prose-blockquote:bg-black/2

                /* 行内代码 */
                prose-code:bg-black/4 prose-code:text-amber-800

                /* 代码块 */
                prose-pre:bg-[#282c34] prose-pre:text-[#abb2bf]

                /* 分隔线 */
                prose-hr:bg-black/6

                /* 表格 */
                [&_table]:bg-black/1
                [&_thead]:bg-black/2
                [&_tbody_tr:nth-child(even)]:bg-black/1.5

                /* 数学公式 */
                [&_.katex-display]:bg-black/2

                /* details */
                [&_details]:bg-black/2
                [&_summary:hover]:bg-black/4

                /* kbd */
                [&_kbd]:bg-black/5

                /* mark */
                [&_mark]:text-amber-900 [&_mark]:bg-amber-400/30

                /* RSS 内容样式 - 浅色主题 */

                /* RSS 引用块 */
                [&_.rss-content-blockquote]:bg-black/2 [&_.rss-content-blockquote]:border-black/10

                /* RSS 代码 */
                [&_.rss-content-pre]:bg-[#282c34] [&_.rss-content-pre]:text-[#abb2bf]
                [&_.rss-content-inline-code]:bg-black/4

                /* RSS 表格 */
                [&_.rss-content-table]:border-black/10
                [&_.rss-content-thead]:bg-black/3
                [&_.rss-content-th]:border-black/10
                [&_.rss-content-td]:border-black/10
                [&_.rss-content-tr:nth-child(even)]:bg-black/1.5

                /* RSS 描述列表 */
                [&_.rss-content-dd]:border-black/10

                /* RSS 折叠 */
                [&_.rss-content-details]:bg-black/2
                [&_.rss-content-summary]:hover:bg-black/4
                [&_.rss-content-details[open]_.rss-content-summary]:border-black/10

                /* RSS kbd */
                [&_.rss-content-kbd]:bg-black/5 [&_.rss-content-kbd]:border-black/10

                /* RSS mark */
                [&_.rss-content-mark]:bg-yellow-200/60

                /* RSS abbr */
                [&_.rss-content-abbr]:border-black/30

                /* RSS 分隔线 */
                [&_.rss-content-hr]:bg-black/10

                /* RSS 类别标签 */
                [&_.rss-content-category]:bg-black/5

                /* RSS 侧边栏 */
                [&_.rss-content-aside]:bg-black/2

                /* Brewlia 注释样式 - 浅色主题 */
                [&_.brewlia-annotation]:cursor-help [&_.brewlia-annotation]:rounded [&_.brewlia-annotation]:px-0.5
                [&_.brewlia-annotation]:transition-all [&_.brewlia-annotation]:duration-200
                [&_.brewlia-annotation]:border-b-2 [&_.brewlia-annotation]:border-dotted
                [&_.brewlia-annotation]:break-words [&_.brewlia-annotation]:[overflow-wrap:anywhere]
                [&_.brewlia-annotation[data-type="term"]]:text-orange-700 [&_.brewlia-annotation[data-type="term"]]:bg-orange-100 [&_.brewlia-annotation[data-type="term"]]:border-orange-400
                [&_.brewlia-annotation[data-type="reference"]]:text-blue-700 [&_.brewlia-annotation[data-type="reference"]]:bg-blue-100 [&_.brewlia-annotation[data-type="reference"]]:border-blue-400
                [&_.brewlia-annotation[data-type="implicit"]]:text-purple-700 [&_.brewlia-annotation[data-type="implicit"]]:bg-purple-100 [&_.brewlia-annotation[data-type="implicit"]]:border-purple-400
                [&_.brewlia-annotation[data-type="context"]]:text-green-700 [&_.brewlia-annotation[data-type="context"]]:bg-green-100 [&_.brewlia-annotation[data-type="context"]]:border-green-400
                [&_.brewlia-annotation[data-type="abbreviation"]]:text-pink-700 [&_.brewlia-annotation[data-type="abbreviation"]]:bg-pink-100 [&_.brewlia-annotation[data-type="abbreviation"]]:border-pink-400
                [&_.brewlia-annotation:hover]:ring-2 [&_.brewlia-annotation:hover]:ring-current/30
                [&_.brewlia-highlight-flash]:animate-pulse [&_.brewlia-highlight-flash]:ring-2 [&_.brewlia-highlight-flash]:ring-purple-500

                /* figcaption */
                prose-figcaption:text-current

                /* Notion 内容样式 - 浅色主题 */

                /* Notion 颜色 - 文字 */
                [&_.notion-gray]:text-gray-500
                [&_.notion-brown]:text-amber-700
                [&_.notion-orange]:text-orange-600
                [&_.notion-yellow]:text-yellow-600
                [&_.notion-green]:text-green-600
                [&_.notion-blue]:text-blue-600
                [&_.notion-purple]:text-purple-600
                [&_.notion-pink]:text-pink-600
                [&_.notion-red]:text-red-600

                /* Notion 颜色 - 背景 */
                [&_.notion-bg-gray]:bg-gray-100 [&_.notion-bg-gray]:px-1 [&_.notion-bg-gray]:rounded
                [&_.notion-bg-brown]:bg-amber-100 [&_.notion-bg-brown]:px-1 [&_.notion-bg-brown]:rounded
                [&_.notion-bg-orange]:bg-orange-100 [&_.notion-bg-orange]:px-1 [&_.notion-bg-orange]:rounded
                [&_.notion-bg-yellow]:bg-yellow-100 [&_.notion-bg-yellow]:px-1 [&_.notion-bg-yellow]:rounded
                [&_.notion-bg-green]:bg-green-100 [&_.notion-bg-green]:px-1 [&_.notion-bg-green]:rounded
                [&_.notion-bg-blue]:bg-blue-100 [&_.notion-bg-blue]:px-1 [&_.notion-bg-blue]:rounded
                [&_.notion-bg-purple]:bg-purple-100 [&_.notion-bg-purple]:px-1 [&_.notion-bg-purple]:rounded
                [&_.notion-bg-pink]:bg-pink-100 [&_.notion-bg-pink]:px-1 [&_.notion-bg-pink]:rounded
                [&_.notion-bg-red]:bg-red-100 [&_.notion-bg-red]:px-1 [&_.notion-bg-red]:rounded

                /* Notion Callout */
                [&_.notion-callout]:flex [&_.notion-callout]:items-start [&_.notion-callout]:gap-3
                [&_.notion-callout]:p-4 [&_.notion-callout]:my-4 [&_.notion-callout]:rounded-xl
                [&_.notion-callout]:bg-black/2 [&_.notion-callout]:border [&_.notion-callout]:border-black/5
                [&_.notion-callout-icon]:text-xl [&_.notion-callout-icon]:shrink-0
                [&_.notion-callout-icon]:w-6 [&_.notion-callout-icon]:h-6 [&_.notion-callout-icon]:object-contain
                [&_.notion-callout-content]:flex-1 [&_.notion-callout-content]:min-w-0

                /* Notion Quote */
                [&_.notion-quote]:pl-4 [&_.notion-quote]:py-1 [&_.notion-quote]:my-4
                [&_.notion-quote]:bg-black/4 [&_.notion-quote]:rounded-xl

                /* Notion Todo */
                [&_.notion-todo]:flex [&_.notion-todo]:items-start [&_.notion-todo]:gap-2 [&_.notion-todo]:my-1
                [&_.notion-checkbox]:w-5 [&_.notion-checkbox]:h-5 [&_.notion-checkbox]:shrink-0
                [&_.notion-checkbox]:border-2 [&_.notion-checkbox]:border-black/20 [&_.notion-checkbox]:rounded
                [&_.notion-checkbox.checked]:bg-blue-500 [&_.notion-checkbox.checked]:border-blue-500
                [&_.notion-checkbox.checked]:after:content-['✓'] [&_.notion-checkbox.checked]:after:text-white
                [&_.notion-checkbox.checked]:after:text-xs [&_.notion-checkbox.checked]:after:flex
                [&_.notion-checkbox.checked]:after:items-center [&_.notion-checkbox.checked]:after:justify-center
                [&_.notion-todo-text.checked]:line-through [&_.notion-todo-text.checked]:opacity-60

                /* Notion Toggle */
                [&_.notion-toggle]:bg-black/2 [&_.notion-toggle]:border [&_.notion-toggle]:border-black/5
                [&_.notion-toggle]:rounded-xl [&_.notion-toggle]:my-3
                [&_.notion-toggle_summary]:px-4 [&_.notion-toggle_summary]:py-3

                /* Notion Image */
                [&_.notion-image]:my-6
                [&_.notion-image_img]:rounded-xl [&_.notion-image_img]:w-full
                [&_.notion-image_figcaption]:text-center [&_.notion-image_figcaption]:text-sm
                [&_.notion-image_figcaption]:mt-2 [&_.notion-image_figcaption]:opacity-60

                /* Notion Video */
                [&_.notion-video]:my-6
                [&_.notion-video-embed]:aspect-video [&_.notion-video-embed]:w-full
                [&_.notion-video-embed]:rounded-xl [&_.notion-video-embed]:overflow-hidden
                [&_.notion-video-embed_iframe]:w-full [&_.notion-video-embed_iframe]:h-full
                [&_.notion-video_video]:w-full [&_.notion-video_video]:rounded-xl

                /* Notion Audio */
                [&_.notion-audio]:my-4
                [&_.notion-audio_audio]:w-full

                /* Notion Bookmark */
                [&_.notion-bookmark]:flex [&_.notion-bookmark]:items-center [&_.notion-bookmark]:gap-3
                [&_.notion-bookmark]:p-4 [&_.notion-bookmark]:my-4 [&_.notion-bookmark]:rounded-xl
                [&_.notion-bookmark]:bg-black/2 [&_.notion-bookmark]:border [&_.notion-bookmark]:border-black/5
                [&_.notion-bookmark]:no-underline [&_.notion-bookmark]:hover:bg-black/4
                [&_.notion-bookmark-icon]:text-lg
                [&_.notion-bookmark-title]:font-medium [&_.notion-bookmark-title]:flex-1
                [&_.notion-bookmark-url]:text-sm [&_.notion-bookmark-url]:opacity-50 [&_.notion-bookmark-url]:truncate [&_.notion-bookmark-url]:max-w-48

                /* Notion Link Preview */
                [&_.notion-link-preview]:inline-flex [&_.notion-link-preview]:items-center [&_.notion-link-preview]:gap-1.5
                [&_.notion-link-preview]:px-2 [&_.notion-link-preview]:py-0.5 [&_.notion-link-preview]:rounded-md
                [&_.notion-link-preview]:bg-black/3 [&_.notion-link-preview]:no-underline
                [&_.notion-link-preview]:hover:bg-black/6

                /* Notion File */
                [&_.notion-file]:inline-flex [&_.notion-file]:items-center [&_.notion-file]:gap-2
                [&_.notion-file]:px-3 [&_.notion-file]:py-2 [&_.notion-file]:my-2 [&_.notion-file]:rounded-lg
                [&_.notion-file]:bg-black/2 [&_.notion-file]:border [&_.notion-file]:border-black/5
                [&_.notion-file]:no-underline [&_.notion-file]:hover:bg-black/4

                /* Notion Embed */
                [&_.notion-embed]:my-6
                [&_.notion-embed-wrapper]:aspect-video [&_.notion-embed-wrapper]:w-full
                [&_.notion-embed-wrapper]:rounded-xl [&_.notion-embed-wrapper]:overflow-hidden
                [&_.notion-embed-wrapper_iframe]:w-full [&_.notion-embed-wrapper_iframe]:h-full

                /* Notion PDF */
                [&_.notion-pdf]:my-6
                [&_.notion-pdf-embed]:w-full [&_.notion-pdf-embed]:h-150 [&_.notion-pdf-embed]:rounded-xl
                [&_.notion-pdf-embed]:border [&_.notion-pdf-embed]:border-black/10

                /* Notion Equation */
                [&_.notion-equation]:my-4 [&_.notion-equation]:py-4 [&_.notion-equation]:px-6
                [&_.notion-equation]:bg-black/2 [&_.notion-equation]:rounded-xl
                [&_.notion-equation]:overflow-x-auto [&_.notion-equation]:text-center

                /* Notion Table */
                [&_.notion-table]:w-full [&_.notion-table]:my-4 [&_.notion-table]:border-collapse
                [&_.notion-table]:rounded-xl [&_.notion-table]:overflow-hidden
                [&_.notion-table_td]:px-3 [&_.notion-table_td]:py-2
                [&_.notion-table_td]:border [&_.notion-table_td]:border-black/10
                [&_.notion-table.has-header_tr:first-child]:bg-black/3
                [&_.notion-table.has-header_tr:first-child_td]:font-medium

                /* Notion Columns */
                [&_.notion-columns]:flex [&_.notion-columns]:gap-4 [&_.notion-columns]:my-4
                [&_.notion-column]:flex-1 [&_.notion-column]:min-w-0

                /* Notion Page Link */
                [&_.notion-page-link]:inline-flex [&_.notion-page-link]:items-center [&_.notion-page-link]:gap-1.5
                [&_.notion-page-link]:px-2 [&_.notion-page-link]:py-1 [&_.notion-page-link]:rounded-md
                [&_.notion-page-link]:bg-black/2 [&_.notion-page-link]:no-underline
                [&_.notion-page-link]:hover:bg-black/4

                /* Notion Code */
                [&_.notion-code]:my-4
                [&_.notion-code_pre]:rounded-xl [&_.notion-code_pre]:overflow-x-auto
                [&_.notion-code_figcaption]:text-center [&_.notion-code_figcaption]:text-sm
                [&_.notion-code_figcaption]:mt-2 [&_.notion-code_figcaption]:opacity-50
              `
              }
            `
}
