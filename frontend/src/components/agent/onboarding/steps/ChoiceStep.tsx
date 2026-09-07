import type { ComponentType, CSSProperties, SVGProps } from 'react'
import type { OnboardingHeaderChrome } from '../onboardingTypes'
import { LuArrowRight, LuSparkles, LuUpload } from '@lib/icons'
import { useLayoutEffect } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import {
  CLOTHING_STYLE_OPTIONS,
  clothingStylePreview,
  GUIDED_MIN_REPORTS,
} from '../onboardingTypes'

type Glyph = ComponentType<SVGProps<SVGSVGElement>>

interface Lane {
  id: 'guided' | 'import'
  icon: Glyph
  title: string
  meta: string
  /** 这条路会产出什么。用各步骤自己的短名，改了步骤这里不会漂。 */
  flow: string[]
  locked?: string
  onPick: () => void
}

interface Props {
  reportCount: number
  onGuided: () => void
  onImport: () => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
}

/**
 * 分岔口。两条路从这里分开，之后不再交叉。
 *
 * 生成是让 AI 从你的报告里长出一个人；导入是你已经有一个人，只想让她住进来。
 * 这两件事对「视觉设定」的处理完全不同——生成会推导出一份视觉设定再据此出图，
 * 导入直接拿现成主图当血统源头、不产出视觉设定——所以放在一条链上会互相污染。
 *
 * 左边上下两条选项；右边单独做角色预览（循环滚过视觉设计的全部服装风格）。
 */
export default function ChoiceStep({
  reportCount,
  onGuided,
  onImport,
  onHeaderChange,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  const canGuide = reportCount >= GUIDED_MIN_REPORTS
  const guidedLock = canGuide
    ? undefined
    : t.config.agentPersonaNeedsReports
        .replace('{count}', String(reportCount))
        .replace('{need}', String(GUIDED_MIN_REPORTS))

  useLayoutEffect(() => {
    onHeaderChange({ description: o.choiceLead })
  }, [o.choiceLead, onHeaderChange])

  const lanes: Lane[] = [
    {
      id: 'guided',
      icon: LuSparkles,
      title: o.choiceGuidedTitle,
      meta: o.choiceGuidedMeta,
      flow: [
        o.step1Short,
        o.step2Short,
        o.step3Short,
        o.step4Short,
        o.step5Short,
      ],
      locked: guidedLock,
      onPick: onGuided,
    },
    {
      id: 'import',
      icon: LuUpload,
      title: o.choiceImportTitle,
      meta: o.choiceImportMeta,
      // 导入直接给到这两样，中间四步都不经过。
      flow: [o.step3Short, o.step5Short],
      onPick: onImport,
    },
  ]

  return (
    <section className="merope-ob-choice" aria-label={o.choiceTitle}>
      <div className="merope-ob-choice__layout">
        <div className="merope-ob-choice__copy">
          <div className="merope-ob-choice__intro">
            <p className="merope-ob-choice__headline">{o.choiceIntroTitle}</p>
            <p className="merope-ob-choice__sub">{o.choiceIntroSub}</p>
          </div>
          <div className="merope-ob-choice__lanes">
            {lanes.map((lane) => {
              const Icon = lane.icon
              return (
                <button
                  key={lane.id}
                  type="button"
                  className="merope-ob-choice__lane"
                  data-lane={lane.id}
                  disabled={Boolean(lane.locked)}
                  aria-label={
                    lane.locked
                      ? `${lane.title} · ${lane.locked}`
                      : `${lane.title} · ${lane.meta}`
                  }
                  onClick={lane.onPick}
                >
                  <span className="merope-ob-choice__badge">
                    <Icon aria-hidden />
                  </span>
                  <span className="merope-ob-choice__title">{lane.title}</span>
                  <span className="merope-ob-choice__go">
                    <LuArrowRight aria-hidden />
                  </span>
                  {lane.locked ? (
                    <span className="merope-ob-choice__lock">{lane.locked}</span>
                  ) : (
                    <span className="merope-ob-choice__flow" aria-hidden>
                      {lane.flow.map((label, index) => (
                        <span key={label} className="merope-ob-choice__chip">
                          <span className="merope-ob-choice__n">
                            {index + 1}
                          </span>
                          {label}
                        </span>
                      ))}
                    </span>
                  )}
                </button>
              )
            })}
          </div>
        </div>
        <div
          className="merope-ob-choice__preview"
          aria-hidden
          style={
            {
              '--choice-reel-n': CLOTHING_STYLE_OPTIONS.length,
            } as CSSProperties
          }
        >
          <span className="merope-ob-choice__reel">
            <span className="merope-ob-choice__track">
              {[0, 1].flatMap((copy) =>
                CLOTHING_STYLE_OPTIONS.map((style) => (
                  <span
                    key={`${copy}-${style}`}
                    className="merope-ob-choice__tile"
                  >
                    <img
                      src={clothingStylePreview(style)}
                      alt=""
                      draggable={false}
                      decoding="async"
                    />
                  </span>
                )),
              )}
            </span>
          </span>
        </div>
      </div>
    </section>
  )
}
