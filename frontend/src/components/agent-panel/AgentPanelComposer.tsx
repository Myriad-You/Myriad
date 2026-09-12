import type { RefObject } from 'react'
import type { MoodBand } from '../agent/meropeVitals'
import type { AgentAttachment, AttachError } from './agentAttachments'
import type { AgentPanelMode } from './agentPanelMode'
import type { ComposerFavorite } from './composerFavorites'
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { useLocation } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import {
  getVoicePresence,
  subscribeVoicePresence,
} from '../../features/merope/speech/voicePresence'
import { agentService } from '../../services/agent'
import { isImeComposing } from '../../utils/ime'
import { AGENT_ATTACH_ACCEPT, collectAttachments } from './agentAttachments'
import { dispatchAgentPanelCommand } from './agentPanelEvents'
import { AgentPanelFace } from './AgentPanelFace'
import {
  AGENT_PANEL_MODES,
  cycleAgentPanelMode,
  useAgentPanelMode,
} from './agentPanelMode'
import { AGENT_SWAP_MS } from './agentPresenceState'
import { agentStatusForLane } from './agentStatus'
import {
  setAgentStatusRecording,
  useAgentLaneLoading,
  useAgentStatus,
} from './agentStatusStore'
import { attachOrbDriftSpeed, startAttachOrbDrift } from './attachOrbDrift'
import { composerActionKind } from './composerAction'
import {
  forgetComposerFavorite,
  loadComposerFavorites,
  subscribeComposerFavorites,
} from './composerFavorites'
import { useAddresseeMoodBand } from './useAddresseeMood'
import { useAgentPanelContext } from './useAgentPanelContext'
import { AgentPresence, AgentPresenceList, AgentSwap } from './useAgentPresence'
import { LONG_PRESS_DURATION } from './useLongPress'
import { useTagStripScroll } from './useTagStripScroll'
import { useVoiceRecording } from './useVoiceRecording'

const FIELD_MAX_PX = 168

function fitComposerField(el: HTMLTextAreaElement | null): void {
  if (!el) return
  if (!el.value) {
    el.style.height = '28px'
    return
  }
  el.style.height = '0px'
  el.style.height = `${Math.min(Math.max(el.scrollHeight, 28), FIELD_MAX_PX)}px`
}

export interface AgentPanelComposerProps {
  onSubmit: (
    text: string,
    attachments?: AgentAttachment[],
    mode?: AgentPanelMode,
  ) => void
  autoFocus?: boolean
  leading?: React.ReactNode
  trailing?: React.ReactNode
}

function ComposerAttach({
  fileRef,
  errorLabel,
}: {
  fileRef: RefObject<HTMLInputElement | null>
  errorLabel: string | null
}) {
  const { t } = useI18n()
  const mode = useAgentPanelMode()
  const { status: island } = useAgentStatus()
  const laneLoading = useAgentLaneLoading(mode)
  const status = agentStatusForLane(island, laneLoading)
  const orbRef = useRef<HTMLSpanElement>(null)
  const orbSpeedRef = useRef(attachOrbDriftSpeed(status))
  orbSpeedRef.current = attachOrbDriftSpeed(status)
  const orbActive = status !== 'idle'
  const attachLabel = `${t.agentPanel.attach.add} · ${t.agentPanel.status[status]}`

  useEffect(() => {
    if (!orbActive) return
    const node = orbRef.current
    if (!node) return
    if (document.documentElement.dataset.perfMode === 'exlight') return
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    return startAttachOrbDrift(node, () => orbSpeedRef.current)
  }, [orbActive])

  return (
    <button
      type="button"
      className="agent-panel-attach"
      data-status={status}
      title={errorLabel ?? attachLabel}
      aria-label={attachLabel}
      onClick={() => fileRef.current?.click()}
    >
      <span className="agent-panel-attach-plus" aria-hidden="true">
        <span className="agent-panel-attach-bar" data-axis="x" />
        <span className="agent-panel-attach-bar" data-axis="y" />
      </span>
      <span ref={orbRef} className="agent-panel-attach-orb" aria-hidden="true">
        <span data-orb-blob="0" />
        <span data-orb-blob="1" />
        <span data-orb-blob="2" />
      </span>
    </button>
  )
}

function ComposerAction({
  hasText,
  hasAttachments,
  value,
  submit,
  speechAvailable,
  isRecording,
  isProcessingVoice,
  conversation,
  toggleRecording,
  enterConversation,
}: {
  hasText: boolean
  hasAttachments: boolean
  value: string
  submit: (text: string) => void
  speechAvailable: boolean
  isRecording: boolean
  isProcessingVoice: boolean
  conversation: boolean
  toggleRecording: () => void
  enterConversation: () => void
}) {
  const { t } = useI18n()
  const mode = useAgentPanelMode()
  const { status } = useAgentStatus()
  const laneLoading = useAgentLaneLoading(mode)
  const display = agentStatusForLane(status, laneLoading)
  const busy = display === 'thinking' || display === 'working'
  const holdTimerRef = useRef<number | null>(null)
  const holdOriginRef = useRef<{ x: number; y: number } | null>(null)
  const holdFiredRef = useRef(false)
  const [holding, setHolding] = useState(false)
  const [speaking, setSpeaking] = useState(false)

  useEffect(() => {
    setAgentStatusRecording(isRecording)
    return () => setAgentStatusRecording(false)
  }, [isRecording])

  useEffect(() => {
    if (!conversation) {
      setSpeaking(false)
      return
    }
    const sync = () => setSpeaking(getVoicePresence().userSpeaking)
    sync()
    return subscribeVoicePresence(sync)
  }, [conversation])

  const clearHold = useCallback(() => {
    if (holdTimerRef.current != null) {
      window.clearTimeout(holdTimerRef.current)
      holdTimerRef.current = null
    }
    holdOriginRef.current = null
  }, [])

  const endHold = useCallback(() => {
    clearHold()
    setHolding(false)
  }, [clearHold])

  useEffect(() => () => clearHold(), [clearHold])

  const kind = composerActionKind({
    hasText,
    hasAttachments,
    busy,
    speechAvailable,
    voiceLocked: isRecording || isProcessingVoice,
    conversation,
  })
  const voiceLabel = conversation
    ? t.agentPanel.voice.conversationStop
    : isProcessingVoice
      ? t.agentPanel.voice.working
      : isRecording
        ? t.agentPanel.voice.stop
        : t.agentPanel.voice.start
  const actionLabel =
    kind === 'stop'
      ? t.agentPanel.stop
      : kind === 'send'
        ? t.agentPanel.send
        : voiceLabel
  const actionTitle =
    kind === 'voice' && !conversation && !isRecording
      ? `${t.agentPanel.voice.start} · ${t.agentPanel.voice.conversationHint}`
      : actionLabel

  return (
    <AgentPresence
      open={!!kind}
      kind="chip"
      from="self"
      durationMs={AGENT_SWAP_MS}
    >
      {kind ? (
        <button
          type="button"
          className={[
            'agent-panel-control',
            'glass',
            kind === 'stop'
              ? 'agent-panel-stop'
              : kind === 'voice'
                ? 'agent-panel-mic'
                : 'agent-panel-send',
          ]
            .filter(Boolean)
            .join(' ')}
          data-on={kind === 'voice' && isRecording ? 'true' : 'false'}
          data-conversation={
            kind === 'voice' && conversation ? 'true' : undefined
          }
          data-holding={kind === 'voice' && holding ? 'true' : undefined}
          data-speaking={kind === 'voice' && speaking ? 'true' : undefined}
          onPointerDown={(event) => {
            if (kind !== 'voice') return
            if (event.button !== 0) return
            holdFiredRef.current = false
            if (conversation) return
            holdOriginRef.current = { x: event.clientX, y: event.clientY }
            event.currentTarget.setPointerCapture(event.pointerId)
            setHolding(true)
            holdTimerRef.current = window.setTimeout(() => {
              holdFiredRef.current = true
              holdTimerRef.current = null
              void enterConversation()
              if (navigator.vibrate) navigator.vibrate(50)
            }, LONG_PRESS_DURATION)
          }}
          onPointerMove={(event) => {
            const origin = holdOriginRef.current
            if (!origin || holdFiredRef.current) return
            if (
              Math.abs(event.clientX - origin.x) > 10 ||
              Math.abs(event.clientY - origin.y) > 10
            ) {
              endHold()
              holdFiredRef.current = false
            }
          }}
          onPointerUp={() => {
            if (kind !== 'voice') {
              endHold()
              return
            }
            const fired = holdFiredRef.current
            holdFiredRef.current = false
            endHold()
            if (fired) return
            toggleRecording()
          }}
          onPointerCancel={() => {
            holdFiredRef.current = false
            endHold()
          }}
          onContextMenu={(event) => {
            if (kind === 'voice') event.preventDefault()
          }}
          onClick={() => {
            if (kind === 'stop') dispatchAgentPanelCommand('interrupt')
            else if (kind === 'send') submit(value)
          }}
          disabled={kind === 'voice' && isProcessingVoice && !conversation}
          title={actionTitle}
          aria-label={actionLabel}
          aria-pressed={kind === 'voice' ? isRecording : undefined}
        >
          {kind === 'voice' ? (
            <span className="agent-panel-mic-fx" aria-hidden="true">
              <span className="agent-panel-mic-hold">
                <svg viewBox="0 0 96 56" preserveAspectRatio="none">
                  <path
                    className="agent-panel-mic-hold-track"
                    pathLength="100"
                    d="M48 3 H68 A25 25 0 0 1 68 53 H28 A25 25 0 0 1 28 3 H48"
                  />
                  <path
                    className="agent-panel-mic-hold-ring"
                    pathLength="100"
                    d="M48 3 H68 A25 25 0 0 1 68 53 H28 A25 25 0 0 1 28 3 H48"
                  />
                </svg>
              </span>
            </span>
          ) : null}
          <AgentSwap id={kind} from="self">
            {kind === 'stop' ? (
              <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                <rect x="7" y="7" width="10" height="10" rx="2" />
              </svg>
            ) : kind === 'send' ? (
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="M12 19V5" />
                <path d="m5.5 11.5 6.5-6.5 6.5 6.5" />
              </svg>
            ) : (
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.9"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <rect x="9" y="3" width="6" height="11" rx="3" />
                <path d="M5 11a7 7 0 0 0 14 0" />
                <path d="M12 18v3" />
              </svg>
            )}
          </AgentSwap>
        </button>
      ) : null}
    </AgentPresence>
  )
}

function ModeTag({ mode }: { mode: AgentPanelMode }) {
  const { t } = useI18n()
  const label = t.agentPanel.mode[mode]
  const shortcut = t.agentPanel.mode.shortcut
  return (
    <button
      type="button"
      className="agent-panel-tag agent-panel-mode"
      aria-label={`${t.agentPanel.mode.label}: ${label}`}
      aria-keyshortcuts={shortcut}
      title={`${label} · ${shortcut}`}
      onClick={() => cycleAgentPanelMode(1)}
    >
      <span className="agent-panel-tag-kicker">{shortcut}</span>
      <span className="agent-panel-mode-labels" aria-hidden="true">
        {AGENT_PANEL_MODES.map((id) => (
          <span key={id} data-on={mode === id ? 'true' : 'false'}>
            {t.agentPanel.mode[id]}
          </span>
        ))}
      </span>
    </button>
  )
}

function MoodTag({ band }: { band: MoodBand }) {
  const { t, format } = useI18n()
  const o = t.agentPersona.onboarding
  const word = o.mood[band]
  return (
    <span
      className="agent-panel-tag agent-panel-mood"
      role="status"
      aria-label={format(o.moodLine, { band: word })}
    >
      <span className="agent-panel-tag-kicker">{t.agentPanel.mood.kicker}</span>
      <span className="agent-panel-tag-text">{word}</span>
    </span>
  )
}

export const AgentPanelComposer: React.FC<AgentPanelComposerProps> = ({
  onSubmit,
  autoFocus = true,
  leading,
  trailing,
}) => {
  const { t, format, locale } = useI18n()
  const mode = useAgentPanelMode()
  const chatting = mode === 'chat'
  const moodBandValue = useAddresseeMoodBand()
  const { pathname } = useLocation()
  const { isAuthenticated } = useAuth()
  const {
    context,
    kicker,
    text: contextText,
    label,
    contextConsent,
    canMute,
    setContextConsent,
  } = useAgentPanelContext(pathname)
  const fieldRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)
  const [value, setValue] = useState('')
  const [attachments, setAttachments] = useState<AgentAttachment[]>([])
  const attachmentsRef = useRef(attachments)
  attachmentsRef.current = attachments
  const [attachError, setAttachError] = useState<AttachError | null>(null)
  const [dropping, setDropping] = useState(false)
  const [favorites, setFavorites] = useState<ComposerFavorite[]>([])
  const tagScrollRef = useTagStripScroll(favorites.length)

  const submit = useCallback(
    (text: string) => {
      const files = attachmentsRef.current
      if (!text.trim() && files.length === 0) return
      setValue('')
      setAttachments([])
      attachmentsRef.current = []
      setAttachError(null)
      onSubmit(text, files.length ? files : undefined)
    },
    [onSubmit],
  )

  const {
    speechAvailable,
    isRecording,
    isProcessingVoice,
    conversation,
    toggleRecording,
    enterConversation,
    stopConversation,
  } = useVoiceRecording((text: string) => {
    if (text.trim() || attachmentsRef.current.length) submit(text)
  }, locale)

  useEffect(() => {
    if (!autoFocus) return
    const timer = setTimeout(() => fieldRef.current?.focus(), 60)
    return () => clearTimeout(timer)
  }, [autoFocus])

  useLayoutEffect(() => {
    fitComposerField(fieldRef.current)
  }, [value])

  useEffect(() => {
    if (!isAuthenticated) return
    let cancelled = false
    const load = () => {
      void (async () => {
        try {
          const next = await loadComposerFavorites()
          if (!cancelled) setFavorites(next)
        } catch {
          /* favorites are optional */
        }
      })()
    }
    load()
    const stop = subscribeComposerFavorites(load)
    return () => {
      cancelled = true
      stop()
    }
  }, [isAuthenticated])

  useEffect(() => {
    if (!attachError) return
    const timer = setTimeout(setAttachError, 2800, null)
    return () => clearTimeout(timer)
  }, [attachError])

  const addFiles = useCallback(async (list: Iterable<File>) => {
    const { attachments: next, error } = await collectAttachments(
      list,
      attachmentsRef.current,
    )
    attachmentsRef.current = next
    setAttachments(next)
    setAttachError(error)
  }, [])

  const removeAttachment = useCallback((id: string) => {
    setAttachments((prev) => {
      const next = prev.filter((item) => item.id !== id)
      attachmentsRef.current = next
      return next
    })
  }, [])

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key !== 'Enter' || event.shiftKey || isImeComposing(event)) {
        return
      }
      event.preventDefault()
      submit(value)
    },
    [submit, value],
  )

  const handlePaste = useCallback(
    (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const files = Iterator.from(event.clipboardData.files).toArray()
      if (!files.length) return
      event.preventDefault()
      void addFiles(files)
    },
    [addFiles],
  )

  const handleDrop = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      event.preventDefault()
      setDropping(false)
      const files = Iterator.from(event.dataTransfer.files).toArray()
      if (files.length) void addFiles(files)
    },
    [addFiles],
  )

  const hasText = value.trim().length > 0
  const hasAttachments = attachments.length > 0
  const attachErrorLabel = attachError ? t.agentPanel.attach[attachError] : null

  useEffect(() => {
    if ((hasText || hasAttachments) && conversation) stopConversation()
  }, [hasText, hasAttachments, conversation, stopConversation])

  const placeholder =
    mode === 'chat'
      ? t.agentPanel.chatPlaceholder
      : t.agentPanel.inputPlaceholder

  return (
    <div className="agent-panel-composer" data-mode={mode}>
      <div className="agent-panel-composer-tags">
        <ModeTag mode={mode} />
        <div className="agent-panel-tag-mid">
          <div
            className="agent-panel-work-chrome"
            ref={tagScrollRef}
            inert={chatting}
          >
            {leading}
            <span className="agent-panel-work-tags">
              <AgentSwap
                id={`${canMute ? 'mute' : 'label'}:${context.kind}`}
                from="self"
              >
                {canMute ? (
                  <button
                    type="button"
                    className="agent-panel-tag"
                    data-tone={contextConsent ? 'neutral' : 'alert'}
                    title={
                      contextConsent
                        ? t.agentPanel.context.muteHint
                        : t.agentPanel.context.allowHint
                    }
                    aria-label={label}
                    aria-pressed={contextConsent}
                    onClick={() => setContextConsent(!contextConsent)}
                  >
                    <span className="agent-panel-tag-kicker">{kicker}</span>
                    <span className="agent-panel-tag-text">{contextText}</span>
                  </button>
                ) : (
                  <span
                    className="agent-panel-tag"
                    data-tone={
                      context.kind === 'selection' ? 'primary' : 'neutral'
                    }
                  >
                    <span className="agent-panel-tag-kicker">{kicker}</span>
                    <span className="agent-panel-tag-text">{contextText}</span>
                  </span>
                )}
              </AgentSwap>
              <AgentPresence
                open={context.kind === 'selection'}
                kind="chip"
                from="context"
              >
                <button
                  type="button"
                  className="agent-panel-tag"
                  onClick={() =>
                    onSubmit(
                      format(t.agentPanel.prompts.explainSelection, {
                        text: context.selection ?? '',
                      }),
                    )
                  }
                >
                  <span className="agent-panel-tag-text">
                    {t.agentPanel.actions.explain}
                  </span>
                </button>
              </AgentPresence>
              <AgentPresence
                open={context.kind === 'selection'}
                kind="chip"
                from="context"
              >
                <button
                  type="button"
                  className="agent-panel-tag"
                  onClick={() =>
                    onSubmit(
                      format(t.agentPanel.prompts.translateSelection, {
                        text: context.selection ?? '',
                      }),
                    )
                  }
                >
                  <span className="agent-panel-tag-text">
                    {t.agentPanel.actions.translate}
                  </span>
                </button>
              </AgentPresence>
              <AgentPresence
                open={context.kind === 'content'}
                kind="chip"
                from="context"
              >
                <button
                  type="button"
                  className="agent-panel-tag"
                  data-tone={chatting ? 'primary' : undefined}
                  onClick={() =>
                    onSubmit(t.agentPanel.prompts.summarize, undefined, 'work')
                  }
                >
                  {chatting ? (
                    <span className="agent-panel-tag-kicker">
                      {t.agentPanel.mode.work}
                    </span>
                  ) : null}
                  <span className="agent-panel-tag-text">
                    {t.agentPanel.actions.summarize}
                  </span>
                </button>
              </AgentPresence>
              <AgentPresence
                open={context.kind === 'content'}
                kind="chip"
                from="context"
              >
                <button
                  type="button"
                  className="agent-panel-tag"
                  data-tone={chatting ? 'primary' : undefined}
                  onClick={() =>
                    onSubmit(t.agentPanel.prompts.translate, undefined, 'work')
                  }
                >
                  {chatting ? (
                    <span className="agent-panel-tag-kicker">
                      {t.agentPanel.mode.work}
                    </span>
                  ) : null}
                  <span className="agent-panel-tag-text">
                    {t.agentPanel.actions.translate}
                  </span>
                </button>
              </AgentPresence>
              <AgentPresenceList
                items={favorites}
                keyOf={(preset) => String(preset.id)}
                kind="chip"
                from="self"
              >
                {(preset) => (
                  <span className="agent-panel-tag agent-panel-tag-saved">
                    <button
                      type="button"
                      className="agent-panel-saved-open"
                      title={preset.input}
                      onClick={() => onSubmit(preset.input)}
                    >
                      <span className="agent-panel-tag-text">
                        {preset.title?.trim() || preset.input}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="agent-panel-tag-dismiss"
                      title={t.agentPanel.unsave}
                      aria-label={t.agentPanel.unsave}
                      onClick={() => {
                        forgetComposerFavorite(preset.id)
                        setFavorites((current) =>
                          current.filter((item) => item.id !== preset.id),
                        )
                        void agentService
                          .toggleFavorite(preset.id)
                          .catch(() => {})
                      }}
                    >
                      <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        aria-hidden="true"
                      >
                        <path d="M6 6l12 12M18 6l-12 12" />
                      </svg>
                    </button>
                  </span>
                )}
              </AgentPresenceList>
            </span>
          </div>
          <AgentPresenceList
            items={attachments}
            keyOf={(item) => item.id}
            kind="chip"
            from="attach"
          >
            {(item) => (
              <span className="agent-panel-tag">
                {item.previewUrl ? <img src={item.previewUrl} alt="" /> : null}
                <span className="agent-panel-tag-kicker">
                  {t.agentPanel.attach.kind}
                </span>
                <span className="agent-panel-tag-text">{item.name}</span>
                <button
                  type="button"
                  className="agent-panel-tag-dismiss"
                  title={t.agentPanel.attach.remove}
                  aria-label={t.agentPanel.attach.remove}
                  onClick={() => removeAttachment(item.id)}
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    aria-hidden="true"
                  >
                    <path d="M6 6l12 12M18 6l-12 12" />
                  </svg>
                </button>
              </span>
            )}
          </AgentPresenceList>
          <AgentPresence open={!!attachErrorLabel} kind="chip" from="attach">
            <span className="agent-panel-tag" data-tone="alert" role="status">
              <span className="agent-panel-tag-text">{attachErrorLabel}</span>
            </span>
          </AgentPresence>
        </div>
        <div className="agent-panel-tag-actions">
          <div className="agent-panel-end-slot">
            <div
              className="agent-panel-end-layer"
              data-on={chatting ? 'false' : 'true'}
              inert={chatting}
            >
              {trailing ? (
                <span className="agent-panel-tag-cluster">{trailing}</span>
              ) : null}
            </div>
            <div
              className="agent-panel-end-layer"
              data-on={chatting ? 'true' : 'false'}
              inert={!chatting}
            >
              {moodBandValue ? <MoodTag band={moodBandValue} /> : null}
            </div>
          </div>
        </div>
      </div>

      <div className="agent-panel-composer-row">
        <div
          className="agent-panel-face-slot"
          aria-hidden={!chatting}
          inert={!chatting}
        >
          <AgentPanelFace playbackEnabled={chatting} />
        </div>
        <div className="agent-panel-field-stage">
          <div
            className="agent-panel-field-shell glass"
            data-drop={dropping ? 'true' : undefined}
            onDragEnter={(event) => {
              event.preventDefault()
              setDropping(true)
            }}
            onDragOver={(event) => {
              event.preventDefault()
            }}
            onDragLeave={(event) => {
              if (event.currentTarget.contains(event.relatedTarget as Node)) {
                return
              }
              setDropping(false)
            }}
            onDrop={handleDrop}
          >
            <ComposerAttach fileRef={fileRef} errorLabel={attachErrorLabel} />
            <textarea
              ref={fieldRef}
              className="agent-panel-field"
              rows={1}
              value={value}
              onChange={(event) => setValue(event.target.value)}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              placeholder={placeholder}
              aria-label={placeholder}
            />
            <input
              ref={fileRef}
              className="agent-panel-attach-input"
              type="file"
              multiple
              accept={AGENT_ATTACH_ACCEPT}
              tabIndex={-1}
              aria-hidden="true"
              onChange={(event) => {
                const files = event.target.files
                if (files?.length) void addFiles(files)
                event.target.value = ''
              }}
            />
          </div>
        </div>

        <ComposerAction
          hasText={hasText}
          hasAttachments={hasAttachments}
          value={value}
          submit={submit}
          speechAvailable={speechAvailable}
          isRecording={isRecording}
          isProcessingVoice={isProcessingVoice}
          conversation={conversation}
          toggleRecording={toggleRecording}
          enterConversation={enterConversation}
        />
      </div>
    </div>
  )
}
