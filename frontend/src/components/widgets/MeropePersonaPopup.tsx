import type { PublicPersonaProfile } from '../../services/agent/agentApi'
import type { MoodBand } from '../agent/meropeVitals'
import type { WidgetPopupProps } from '../widgetGridTypes'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n, withI18nNamespace } from '../../contexts/I18nContext'
import { PERSONA_UPDATED_EVENT } from '../../features/merope/events/updates'
import { PERSONA_OFF_NAME } from '../../features/merope/persona/publicName'
import {
  releaseFigureStage,
  showFigureOnStage,
} from '../../features/merope/presence/figureStage'
import { getWidgetPopupState, subscribeWidgetPopupState } from '../../lib/widgetPopup'
import { agentService } from '../../services/agent'
import { useAgentStatusKind } from '../agent-panel/agentStatusStore'
import { moodBand } from '../agent/meropeVitals'
import { MeropeHeadSkeleton } from './MeropeSkeleton'
import { useMeropePersona } from './useMeropePersona'
import './MeropePersonaPopup.css'

const MOOD_LEVEL: Record<MoodBand, number> = {
  floor: 1,
  sad: 2,
  tense: 2,
  calm: 3,
  excited: 4,
}
const LEVEL_SLOTS = [0, 1, 2, 3]

type ActivityKey = 'idle' | 'working' | 'thinking' | 'talking'

function activityOf(status: string): ActivityKey {
  if (status === 'working' || status === 'thinking') return status
  if (status === 'listening' || status === 'needsInput') return 'talking'
  return 'idle'
}

/**
 * 弹窗在前台时把舞台借给打开它的那张卡，卡片把她（同一个播放器）挪过来；
 * 一收回就同步还回去，赶在宿主复制卡片画面之前，落地时卡片上已经是她。
 * 弹窗的 key 就是卡片 id（没有 shareKey）。
 */
function useFigureStage(ownerId: string) {
  const stageRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const element = stageRef.current
    if (!element) return undefined
    const sync = () => {
      if (getWidgetPopupState().activeKey === ownerId) showFigureOnStage(ownerId, element)
      else releaseFigureStage(ownerId, element)
    }
    sync()
    const unsubscribe = subscribeWidgetPopupState(sync)
    return () => {
      unsubscribe()
      releaseFigureStage(ownerId, element)
    }
  }, [ownerId])
  return stageRef
}

const SENTENCE = /[^。！？!?]+[。！？!?]*/g

/**
 * 简介只有两行：能放下几句整句就放几句，不在半句上截断。
 * 用一个看不见的同款段落量，第一句都放不下才交给 line-clamp 的省略号。
 */
function useFittedSummary(summary: string) {
  const ref = useRef<HTMLParagraphElement>(null)
  const [count, setCount] = useState<number | null>(null)
  const sentences = summary.match(SENTENCE) ?? []

  useLayoutEffect(() => {
    const element = ref.current
    if (!element || !summary) return undefined
    const parts = summary.match(SENTENCE) ?? []
    const fit = () => {
      const probe = element.cloneNode(false) as HTMLParagraphElement
      probe.removeAttribute('title')
      probe.style.cssText = `position:absolute;visibility:hidden;pointer-events:none;width:${element.clientWidth}px`
      element.parentElement?.append(probe)
      let next = 1
      for (let n = parts.length; n > 1; n -= 1) {
        probe.textContent = parts.slice(0, n).join('')
        if (probe.scrollHeight <= probe.clientHeight + 1) {
          next = n
          break
        }
      }
      probe.remove()
      setCount(next)
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(element)
    return () => observer.disconnect()
  }, [summary])

  const text = count === null ? summary : sentences.slice(0, count).join('')
  return { ref, text }
}

/**
 * 公开的人设正文：所有人看到的是同一份，站长也不多看。
 * undefined 是还没读到；读到了没有、或者读失败都是 null。
 */
function usePublicProfile(): PublicPersonaProfile | null | undefined {
  const [profile, setProfile] = useState<PublicPersonaProfile | null | undefined>(undefined)
  useEffect(() => {
    let alive = true
    const load = () => {
      void agentService
        .getPublicPersona()
        .then((data) => {
          if (alive) setProfile(data.profile ?? null)
        })
        .catch(() => {
          if (alive) setProfile((current) => current ?? null)
        })
    }
    load()
    window.addEventListener(PERSONA_UPDATED_EVENT, load)
    return () => {
      alive = false
      window.removeEventListener(PERSONA_UPDATED_EVENT, load)
    }
  }, [])
  return profile
}

/**
 * 人设卡的弹窗，主要给访客看：会动的她占大半，名字、此刻的状态和人设靠边站。
 */
function MeropePersonaPopupBody({ config, active }: WidgetPopupProps) {
  const { t, format } = useI18n()
  const o = t.agentPersona.onboarding
  const { isAdmin } = useAuth()
  const { name, mood, arousal, vitalsReady } = useMeropePersona()
  const profile = usePublicProfile()
  const stageRef = useFigureStage(config.id)
  const activity = activityOf(useAgentStatusKind())
  const band = vitalsReady ? moodBand(mood, arousal) : null

  const summary = profile?.summary.trim() ?? ''
  const fitted = useFittedSummary(summary)

  return (
    <div
      className="merope-profile"
      data-active={active || undefined}
    >
      <div className="merope-profile__stage">
        <div className="merope-widget merope-widget--stage">
          <div ref={stageRef} className="merope-widget__surface" />
        </div>
      </div>

      <div className="merope-profile__info">
        <header className="merope-profile__head">
          {name === null ? (
            <MeropeHeadSkeleton label={t.common.loading} />
          ) : (
            <>
          <h2 className="merope-profile__name">{name}</h2>
          <div className="merope-profile__status">
            <span className="merope-profile__pill" data-activity={activity}>
              <i className="merope-profile__dot" aria-hidden />
              {o.activity[activity]}
            </span>
            {band ? (
              <span className="merope-profile__pill">
                {format(o.moodLine, { band: o.mood[band] })}
                <span className="merope-profile__level" aria-hidden>
                  {LEVEL_SLOTS.map((slot) => (
                    <i key={slot} data-on={slot < MOOD_LEVEL[band] || undefined} />
                  ))}
                </span>
              </span>
            ) : null}
          </div>
          {profile === null && isAdmin && name !== PERSONA_OFF_NAME ? (
            <p className="merope-profile__hint">{t.merope.profileNoPersona}</p>
          ) : null}
            </>
          )}
        </header>

        {summary ? (
          <p ref={fitted.ref} className="merope-profile__summary" title={summary}>
            {fitted.text}
          </p>
        ) : null}
      </div>
    </div>
  )
}

export default withI18nNamespace(['merope'], MeropePersonaPopupBody)
