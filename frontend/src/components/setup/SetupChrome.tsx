/**
 * 初始化向导的外壳原语 —— 设定引导也用同一套排版。
 *
 * 顶栏（返回 + 步骤位置）→ 标题区（标题 / 副标题 / 提示）→ 正文 → 右下悬浮动作。
 * 没有进度条：你在哪一步全靠顶栏右侧那一行文字交代。
 */

import type {
  ComponentType,
  InputHTMLAttributes,
  ReactNode,
  RefObject,
  SVGProps,
} from 'react'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuChevronLeft,
  LuInfo,
  LuLoader2,
  LuSettings,
} from '@lib/icons'
import { useEffect } from 'react'

type Glyph = ComponentType<SVGProps<SVGSVGElement>>

/** 玻璃背后的漂移光斑：backdrop-filter 需要可折射的内容才显得出玻璃。 */
export function Aurora() {
  return (
    <div className="setup-ob__aurora" aria-hidden>
      <i />
      <i />
    </div>
  )
}

/**
 * 顶栏色层浓度：随正文滚动 0→1（smoothstep）。
 * 写在卡片根上的 --sob-top-dense，CSS 用它混合顶栏 tint，
 * 让内容滚到顶栏底下时不会和步骤文字糊在一起。
 */
export function useTopBarDense(
  cardRef: RefObject<HTMLElement | null>,
  scrollerRef: RefObject<HTMLElement | null>,
  /** 换步时重新绑定滚动容器 */
  key: string,
): void {
  useEffect(() => {
    const card = cardRef.current
    const scroller = scrollerRef.current
    if (!card) return undefined
    if (!scroller) {
      card.style.setProperty('--sob-top-dense', '0')
      return undefined
    }

    const syncDense = () => {
      const t = Math.min(1, Math.max(0, scroller.scrollTop / 128))
      const dense = t * t * (3 - 2 * t)
      card.style.setProperty('--sob-top-dense', dense.toFixed(3))
    }

    syncDense()
    scroller.addEventListener('scroll', syncDense, { passive: true })
    return () => {
      scroller.removeEventListener('scroll', syncDense)
      card.style.setProperty('--sob-top-dense', '0')
    }
  }, [cardRef, scrollerRef, key])
}

/**
 * 顶部条：左边「返回上一步」，右边一行淡淡的当前步骤名与位置。
 * 状态页（读取中 / 已完成）不传 stepName，右侧那行就整条不画。
 */
export function StepTopBar({
  back,
  stepName,
  current,
  total,
  progressText,
}: {
  back?: ReactNode
  stepName?: string
  current?: number
  total?: number
  progressText?: string
}) {
  return (
    <div className="setup-ob-top">
      <div className="setup-ob-top__side" key={stepName ?? 'brand'}>
        {back}
      </div>
      {stepName && current && total ? (
        <p
          className="setup-ob-top__step"
          aria-label={progressText}
          key={`${current}-${stepName}`}
        >
          <b>{stepName}</b>
          <span aria-hidden>
            {current}/{total}
          </span>
        </p>
      ) : null}
    </div>
  )
}

/**
 * 左上角返回：只有圆底箭头可点，旁挂上一页名称（只展示、不吃点击）。
 * 完整「返回 xxx」写在按钮的 aria-label / title 上。
 */
export function BackButton({
  label,
  destination,
  disabled,
  onClick,
}: {
  /** 读屏 / tooltip 用的完整返回文案，例如「返回 欢迎」 */
  label: string
  /** 按钮旁可见的上一页名称，例如「欢迎」 */
  destination: string
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <div className="setup-ob-back">
      <button
        type="button"
        className="setup-ob-back__hit"
        disabled={disabled}
        title={label}
        aria-label={label}
        onClick={onClick}
      >
        <LuChevronLeft aria-hidden />
      </button>
      <span className="setup-ob-back__label" aria-hidden>
        {destination}
      </span>
    </div>
  )
}

/**
 * 无处可返回的那几步（首屏 / 终态）的左上角占位：
 * 与返回按钮同一副骨架——圆底图标 + 旁挂文字，只是图标换成齿轮且不吃点击。
 * 这样换步时左上角的重心不会跳。
 */
export function BrandMark({
  label,
  icon: Icon = LuSettings,
}: {
  label: string
  icon?: Glyph
}) {
  return (
    <div className="setup-ob-back is-static">
      <span className="setup-ob-back__hit" aria-hidden>
        <Icon />
      </span>
      <span className="setup-ob-back__label">{label}</span>
    </div>
  )
}

/** 没有可返回的地方时，用同款药丸标签占住左上角。 */
export function BrandTag({
  label,
  showLogo = true,
}: {
  label: string
  showLogo?: boolean
}) {
  return (
    <span className={`setup-ob-brand${showLogo ? '' : ' is-textonly'}`}>
      {showLogo ? <img src="/logo.webp" alt="" aria-hidden /> : null}
      {label}
    </span>
  )
}

/**
 * 每一步统一的标题区：（可选眉标 →）标题 → 副标题(lead) → 可选 notes。
 * notes 只放操作提示与阶段性状态；字段旁的小字 hint 仍挂在 Field 上。
 */
export function StepHero({
  eyebrow,
  title,
  lead,
  titleId,
  action,
  notes,
}: {
  eyebrow?: string
  /** 大多数步骤是一句纯文本；完成屏用两行（问候 + 提示）时传 ReactNode */
  title: ReactNode
  lead: string
  titleId: string
  action?: ReactNode
  notes?: ReactNode
}) {
  return (
    <header className="setup-ob-hero">
      {eyebrow ? <p className="setup-ob-hero__eyebrow">{eyebrow}</p> : null}
      <div className="setup-ob-hero__row">
        <h1 id={titleId}>{title}</h1>
        {action}
      </div>
      <p className="setup-ob-hero__lead">{lead}</p>
      {notes ? <div className="setup-ob-hero__notes">{notes}</div> : null}
    </header>
  )
}

/** 步骤正文容器：控制间距与子块入场节奏。 */
export function StepBody({ children }: { children: ReactNode }) {
  return <div className="setup-ob-body">{children}</div>
}

/** 底部操作条：只放向前的动作，返回已经在左上角。 */
/**
 * split: 左边留一句轻提示，右边仍是浮起的主动作——
 * 目前只有欢迎屏用这个变体，配一句「通常只需几分钟」。
 */
export function ActionBar({
  children,
  split = false,
}: {
  children: ReactNode
  split?: boolean
}) {
  return (
    <footer className={`setup-ob-bar${split ? ' is-split' : ''}`}>
      {children}
    </footer>
  )
}

/**
 * 主动作。进了 ActionBar 会收成右下角那颗浮起的圆钮（文案退给读屏与 tooltip），
 * 留在正文里则是一颗文案可见的药丸。
 */
export function PrimaryButton({
  label,
  icon: Icon = LuArrowRight,
  busy = false,
  disabled = false,
  type = 'button',
  onClick,
}: {
  label: string
  /** 默认向前箭头；正文里的动作传 null 表示不要图标。 */
  icon?: Glyph | null
  busy?: boolean
  disabled?: boolean
  type?: 'button' | 'submit'
  onClick?: () => void
}) {
  return (
    <button
      type={type}
      className="setup-ob-cta"
      disabled={disabled || busy}
      title={label}
      aria-label={label}
      onClick={onClick}
    >
      <span>{label}</span>
      {busy ? (
        <LuLoader2 className="setup-ob-spin" aria-hidden />
      ) : Icon ? (
        <Icon aria-hidden />
      ) : null}
    </button>
  )
}

/** 次要动作：同一层玻璃，但不吸主题色。 */
export function GhostButton({
  label,
  icon: Icon,
  busy = false,
  disabled = false,
  onClick,
}: {
  label: string
  icon?: Glyph
  busy?: boolean
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="setup-ob-ghost"
      disabled={disabled || busy}
      onClick={onClick}
    >
      {busy ? (
        <LuLoader2 className="setup-ob-spin" aria-hidden />
      ) : Icon ? (
        <Icon aria-hidden />
      ) : null}
      {label}
    </button>
  )
}

export type NoteTone = 'info' | 'warn' | 'success' | 'error' | 'active'

/** 一句提示。info = 顺带说明，warn = 需要你处理，error / success = 动作结果。 */
export function Note({
  tone = 'info',
  children,
}: {
  tone?: NoteTone
  children: ReactNode
}) {
  const Icon =
    tone === 'warn' || tone === 'error'
      ? LuAlertTriangle
      : tone === 'success'
        ? LuCheck
        : LuInfo
  return (
    <p
      className={`setup-ob-note is-${tone}`}
      role={tone === 'error' ? 'alert' : 'status'}
      aria-live="polite"
    >
      <Icon aria-hidden />
      <span>{children}</span>
    </p>
  )
}

/** 表单字段：一行标签 +（可选）「可选」徽标 + 控件 +（可选）小字说明。 */
export function Field({
  label,
  hint,
  optional,
  optionalLabel,
  wide,
  children,
}: {
  label: string
  hint?: ReactNode
  optional?: boolean
  optionalLabel?: string
  /** 在两列网格里独占整行 */
  wide?: boolean
  children: ReactNode
}) {
  return (
    <label className={`setup-ob-field${wide ? ' is-wide' : ''}`}>
      <span className="setup-ob-field__label">
        {label}
        {optional && optionalLabel && (
          <i className="setup-ob-field__optional">{optionalLabel}</i>
        )}
      </span>
      {children}
      {hint && <small className="setup-ob-field__hint">{hint}</small>}
    </label>
  )
}

export function TextInput({
  mono,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return (
    <input className={`setup-ob-input${mono ? ' is-mono' : ''}`} {...props} />
  )
}
