/**
 * 轮换小组件的页码点：看得出有几页、在第几页，点了直接去那一页。
 * 平时收起；悬停、键盘聚焦、刚手动拨过时露出。触屏不常显：会一直压着卡片底部的内容，
 * 触屏用户会先试着滑，滑过之后它就露出来。
 * 自带磨砂底，免得和卡片里的进度条、圆点混在一起。
 */

import { useI18n } from '../../../contexts/I18nContext'

function cx(...parts: (string | false | undefined)[]): string {
  return parts.filter(Boolean).join(' ')
}

export interface WidgetPagerProps {
  count: number
  index: number
  visible: boolean
  onSelect: (index: number) => void
  /** 默认贴底；底部有固定内容的卡（报告卡左下角的平台名）放顶上。 */
  placement?: 'top' | 'bottom'
  className?: string
}

export function WidgetPager({
  count,
  index,
  visible,
  onSelect,
  placement = 'bottom',
  className,
}: WidgetPagerProps) {
  const { t, format } = useI18n()
  if (count < 2) return null
  return (
    <div
      role="group"
      aria-label={t.widgetGrid.pagerLabel}
      data-rotation-ignore=""
      className={cx(
        'absolute left-1/2 z-30 flex -translate-x-1/2 items-center rounded-full bg-white/70 px-1 shadow-[0_1px_4px_rgb(0_0_0/12%)] ring-1 ring-black/5 backdrop-blur-md transition-opacity duration-200 dark:bg-neutral-900/70 dark:ring-white/10',
        // 收起时不挡点击；键盘 Tab 进来会触发 focus 让它露出。
        visible
          ? 'opacity-100'
          : 'pointer-events-none opacity-0 focus-within:pointer-events-auto focus-within:opacity-100',
        placement === 'top' ? 'top-1.5' : 'bottom-1.5',
        className,
      )}
      onClick={(event) => event.stopPropagation()}
    >
      {Array.from({ length: count }, (_, i) => {
        const current = i === index
        return (
          <button
            key={i}
            type="button"
            aria-label={format(t.widgetGrid.goToPage, {
              page: i + 1,
              total: count,
            })}
            aria-current={current ? 'true' : undefined}
            className={cx(
              'group/dot flex h-4 items-center justify-center outline-none',
              // 页多时收紧点距，十来页也不至于横跨半张卡。
              count > 6 ? 'px-[2px]' : 'px-[3px]',
            )}
            onClick={(event) => {
              event.preventDefault()
              event.stopPropagation()
              if (!current) onSelect(i)
            }}
          >
            <span
              className={cx(
                'block h-[5px] rounded-full transition-all duration-300 group-focus-visible/dot:ring-2 group-focus-visible/dot:ring-[var(--cfg-accent)]',
                current
                  ? 'w-3.5 bg-[var(--cfg-accent)]'
                  : 'w-[5px] bg-gray-500/45 group-hover/dot:bg-gray-500/80 dark:bg-white/45 dark:group-hover/dot:bg-white/80',
              )}
            />
          </button>
        )
      })}
    </div>
  )
}
