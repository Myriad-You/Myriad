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

const TAG_FIELDS = ['temperament', 'likes', 'drives'] as const
const LINE_FIELDS = ['socialStyle', 'speechStyle'] as const

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

  const labels = {
    temperament: o.fieldTemperament,
    likes: o.fieldLikes,
    drives: o.fieldDrives,
    socialStyle: o.fieldSocial,
    speechStyle: o.fieldVoice,
  }
  const tags = profile ? TAG_FIELDS.filter((key) => profile[key].length > 0) : []
  const lines = profile ? LINE_FIELDS.filter((key) => profile[key].trim()) : []
  const summary = profile?.summary.trim() ?? ''

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

        {summary ? <p className="merope-profile__summary">{summary}</p> : null}

        {profile && (tags.length || lines.length) ? (
          <dl className="merope-profile__facts">
            {tags.map((key) => (
              <div key={key} className="merope-profile__fact">
                <dt>{labels[key]}</dt>
                <dd>
                  <ul className="merope-profile__tags">
                    {profile[key].map((item) => (
                      <li key={item}>{item}</li>
                    ))}
                  </ul>
                </dd>
              </div>
            ))}
            {lines.map((key) => (
              <div key={key} className="merope-profile__fact">
                <dt>{labels[key]}</dt>
                <dd>{profile[key].trim()}</dd>
              </div>
            ))}
          </dl>
        ) : null}

      </div>
    </div>
  )
}

export default withI18nNamespace(['merope'], MeropePersonaPopupBody)
