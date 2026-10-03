/**
 * 轮换小组件的页码：看得出有几页、在第几页，点了直接去那一页。
 * 平时收起；悬停、键盘聚焦、刚手动拨过时露出。触屏不常显：会一直压着卡片底部的内容，
 * 触屏用户会先试着滑，滑过之后它就露出来。
 * 自带磨砂底，免得和卡片里的进度条、圆点混在一起。
 *
 * 点放得下就用点；放不下（小卡片、页数多，比如 2×2 友链一批一个）改成「‹ 2 / 24 ›」，
 * 否则一排点会溢出卡片、被裁掉，也点不到。
 */

import { LuChevronLeft, LuChevronRight, LuPause, LuPlay } from '@lib/icons'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { pagerDotCapacity, TOGGLE_PX } from './pagerCapacity'

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
  /** 自动轮换被用户停掉了；传了 onToggleStopped 才显示暂停/继续按钮。 */
  stopped?: boolean
  onToggleStopped?: () => void
  className?: string
}

const STEP_BUTTON =
  'flex h-4 w-4 items-center justify-center rounded-full text-gray-600 outline-none hover:text-gray-900 focus-visible:ring-2 focus-visible:ring-[var(--cfg-accent)] dark:text-white/70 dark:hover:text-white'

export function WidgetPager({
  count,
  index,
  visible,
  onSelect,
  placement = 'bottom',
  stopped = false,
  onToggleStopped,
  className,
}: WidgetPagerProps) {
  const { t, format } = useI18n()
  const ref = useRef<HTMLDivElement | null>(null)
  // 以定位参照（left: 50% 相对的那个元素）的宽度为准，不能用父元素：报告卡里父元素是
  // display: contents，宽度是 0。量到之前先按点排（大多数卡放得下）。
  const [capacity, setCapacity] = useState(Number.POSITIVE_INFINITY)
  const shown = count >= 2
  const withToggle = !!onToggleStopped
  useLayoutEffect(() => {
    const el = ref.current
    const host = (el?.offsetParent as HTMLElement | null) ?? el?.parentElement
    if (!shown || !host) return
    // 暂停按钮也占一格宽度。
    const reserved = withToggle ? TOGGLE_PX : 0
    const measure = () => setCapacity(pagerDotCapacity(host.clientWidth - reserved))
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(host)
    return () => observer.disconnect()
  }, [shown, withToggle])

  // 只有当前点进 Tab 序列（← → 翻页）；翻页后焦点跟到新的当前点，不留在已经不是当前的点上。
  useEffect(() => {
    const el = ref.current
    if (!el || !el.contains(document.activeElement)) return
    const focused = document.activeElement as HTMLElement
    if (!focused.hasAttribute('data-pager-dot')) return
    el.querySelector<HTMLElement>('[data-pager-dot][aria-current]')?.focus({
      preventScroll: true,
    })
  }, [index])

  if (!shown) return null
  const compact = count > capacity
  const go = (next: number) => onSelect(((next % count) + count) % count)

  return (
    <div
      ref={ref}
      role="group"
      aria-label={t.widgetGrid.pagerLabel}
      data-rotation-ignore=""
      className={cx(
        'absolute left-1/2 z-30 flex -translate-x-1/2 items-center rounded-full bg-white/70 px-1 shadow-[0_1px_4px_rgb(0_0_0/12%)] ring-1 ring-black/5 backdrop-blur-md transition-opacity duration-200 dark:bg-neutral-900/70 dark:ring-white/10',
        // 收起时不挡点击；键盘 Tab 进来会触发 focus 让它露出。
        // 只认键盘焦点：focus-within 在安卓上点过页码点后一直成立，胶囊收不起来、还挡着点击。
        visible
          ? 'opacity-100'
          : 'pointer-events-none opacity-0 has-[:focus-visible]:pointer-events-auto has-[:focus-visible]:opacity-100',
        placement === 'top' ? 'top-1.5' : 'bottom-1.5',
        className,
      )}
      onClick={(event) => event.stopPropagation()}
    >
      {compact ? (
        <>
          <button
            type="button"
            aria-label={t.widgetGrid.prevPage}
            className={STEP_BUTTON}
            onClick={(event) => {
              event.preventDefault()
              event.stopPropagation()
              go(index - 1)
            }}
          >
            <LuChevronLeft className="h-3 w-3" aria-hidden />
          </button>
          <span
            aria-current="page"
            aria-label={format(t.widgetGrid.goToPage, {
              page: index + 1,
              total: count,
            })}
            className="min-w-10 px-0.5 text-center text-[10px] font-semibold leading-4 tabular-nums text-gray-700 dark:text-white/80"
          >
            {`${index + 1} / ${count}`}
          </span>
          <button
            type="button"
            aria-label={t.widgetGrid.nextPage}
            className={STEP_BUTTON}
            onClick={(event) => {
              event.preventDefault()
              event.stopPropagation()
              go(index + 1)
            }}
          >
            <LuChevronRight className="h-3 w-3" aria-hidden />
          </button>
        </>
      ) : (
        Array.from({ length: count }, (_, i) => {
          const current = i === index
          return (
            <button
              key={i}
              type="button"
              data-pager-dot=""
              tabIndex={current ? 0 : -1}
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
                  // 当前点自己就是强调色，焦点环要隔开一圈才看得出来。
                  'block h-[5px] rounded-full transition-all duration-300 group-focus-visible/dot:ring-2 group-focus-visible/dot:ring-[var(--cfg-accent)] group-focus-visible/dot:ring-offset-1 group-focus-visible/dot:ring-offset-white dark:group-focus-visible/dot:ring-offset-neutral-900',
                  current
                    ? 'w-3.5 bg-[var(--cfg-accent)]'
                    : 'w-[5px] bg-gray-500/45 group-hover/dot:bg-gray-500/80 dark:bg-white/45 dark:group-hover/dot:bg-white/80',
                )}
              />
            </button>
          )
        })
      )}
      {onToggleStopped ? (
        <button
          type="button"
          aria-label={stopped ? t.widgetGrid.resumeRotation : t.widgetGrid.pauseRotation}
          aria-pressed={stopped}
          className={cx(STEP_BUTTON, 'ml-0.5')}
          onClick={(event) => {
            event.preventDefault()
            event.stopPropagation()
            onToggleStopped()
          }}
        >
          {stopped ? (
            <LuPlay className="h-2.5 w-2.5" aria-hidden />
          ) : (
            <LuPause className="h-2.5 w-2.5" aria-hidden />
          )}
        </button>
      ) : null}
    </div>
  )
}
