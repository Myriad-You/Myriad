import type { ReactNode } from 'react'
import { LuLoader2, LuSearch } from '@lib/icons'

/**
 * 工具窗口的内容骨架。窗口标题栏已经写着工具名和图标，这里只留一行工具条：
 * 返回（进了下一层时才有，连着当前层的名字）、搜索、动作按钮；筛选另起一行，下面是内容。
 */
export function WorkbenchPage({
  title,
  action,
  search,
  filters,
  back,
  children,
}: {
  title: string
  action?: ReactNode
  search?: ReactNode
  filters?: ReactNode
  back: ReactNode
  children: ReactNode
}) {
  const crumb = back ? (
    <div className="tool-pane__crumb">
      {back}
      <h2 className="tool-pane__crumb-title">{title}</h2>
    </div>
  ) : null
  const bar = crumb || search || action || filters

  return (
    <section className="tool-pane" aria-label={title}>
      {bar ? (
        <div className="tool-pane__bar">
          {crumb}
          {search ? <div className="tool-pane__search-slot">{search}</div> : null}
          {action ? <div className="tool-pane__actions">{action}</div> : null}
          {filters ? <div className="tool-pane__filters">{filters}</div> : null}
        </div>
      ) : null}
      <div className="tool-pane__body">{children}</div>
    </section>
  )
}

/** 工具条里的搜索框。 */
export function WorkbenchSearch({
  value,
  onChange,
  placeholder,
}: {
  value: string
  onChange: (value: string) => void
  placeholder: string
}) {
  return (
    <label className="tool-pane__search">
      <LuSearch aria-hidden />
      <input
        type="search"
        value={value}
        placeholder={placeholder}
        aria-label={placeholder}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  )
}

/** 工具条里的动作按钮：图标加短名，说明放进悬停提示。 */
export function PageAction({
  label,
  description,
  icon,
  disabled,
  loading,
  primary,
  onPick,
}: {
  label: string
  description?: string
  icon: ReactNode
  disabled?: boolean
  loading?: boolean
  primary?: boolean
  onPick: () => void
}) {
  return (
    <button
      type="button"
      className={`tool-pane__action${primary ? ' is-primary' : ''}`}
      title={description}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      onClick={onPick}
    >
      <span className="tool-pane__action-icon" aria-hidden>
        {loading ? <LuLoader2 className="tool-pane__spin" /> : icon}
      </span>
      <span>{label}</span>
    </button>
  )
}
