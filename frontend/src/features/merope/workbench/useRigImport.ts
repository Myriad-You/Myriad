import type { RigPreviewRetry } from '../assets/compiler'
import type {
  RigAssetCompileEvent,
  RigAssetPreflight,
} from '../assets/pipeline'
import type { TurnArchiveUse, TurnKeyformsAsked, TurnKeyformsBeside, TurnKeyformsStatus } from '../turnKeyformsApi'
import { useEffect, useRef, useState } from 'react'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import { useI18n } from '../../../contexts/I18nContext'
import { showStickyToast } from '../../../utils/toastManager'
import { userFacingError } from '../../../utils/userFacingError'
import { rigPreviewRetryOf } from '../assets/compiler'
import { optionalDrawing } from './optionalDrawing'
import { besideOfPickedUp, boxesOfPickedUp } from './pickedUpSplit'

export type RigImportStepState = Partial<
  Record<RigAssetCompileEvent['stage'], RigAssetCompileEvent['status']>
>

export interface RigImportSummary {
  partCount: number
  score: number
  activated: boolean
}

/**
 * The plain rig: an uploaded PSD, its turn worked out from the front drawing
 * (11°). The enhanced one is split by See-through and adds what is
 * generated: turn keys fitted to four turned drawings (about 30°), for a
 * standing figure its body's turn and weight shift, for the bust the
 * AI-redrawn expressions; each can be chosen on its own.
 */
export type RigMode = 'plain' | 'enhanced'

/** What a standing figure's enhancement keys: its head's turn, its body's turn and weight shift. */
export interface EnhanceParts {
  head: boolean
  body: boolean
}

export interface RigImportSource {
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  /**
   * Splits the portrait with turn keys; with `fromArchive`, uses that kept job
   * instead (fitted again, or as it was). A standing figure keys the `parts` chosen.
   * `beside` (what goes on the result with the keys) is kept by the job for a
   * page that picks it up.
   */
  onDecomposeRigPsd: (
    onStatus: (status: TurnKeyformsStatus) => void,
    signal: AbortSignal,
    fromArchive?: TurnArchiveUse,
    parts?: EnhanceParts,
    beside?: TurnKeyformsBeside,
  ) => Promise<File>
  /**
   * The split the server has for this portrait with no page of this one's
   * making (it was reloaded, or opened in another tab; or the split ended
   * with no page there), picked up: its file once done, or null when there
   * is none. A failure no page was told is thrown.
   */
  onResumeRigPsd?: (
    onStatus: (status: TurnKeyformsStatus) => void,
    signal: AbortSignal,
  ) => Promise<File | null>
  /** Splits the portrait once without keys: the base the bust's redrawn expressions go on when its turn is not keyed. */
  onDecomposePlainPsd?: (
    signal: AbortSignal,
    onStatus: (status: TurnKeyformsStatus) => void,
    beside?: TurnKeyformsBeside,
  ) => Promise<File>
  /** A standing figure: its body's turn and weight shift can be keyed beside its head. */
  bodyKeysOffered?: boolean
  onPreflightRigPsd: (
    file: File,
    onStage: (event: RigAssetCompileEvent) => void,
    signal?: AbortSignal,
    options?: { aiExpressions?: boolean; armVariants?: boolean },
  ) => Promise<RigAssetPreflight>
  onCommitRigPsd: (
    preflight: RigAssetPreflight,
    onStage: (event: RigAssetCompileEvent) => void,
  ) => Promise<{ partCount: number; score: number }>
  /** Redraws the expressions with the image model; absent where they are not offered (full body). */
  onGenerateAiExpressions?: () => Promise<void>
  /** Whether redrawn expressions exist for the current portrait already. */
  aiExpressionsReady?: boolean
  /** Expressions being drawn on the server, begun before a reload or in another tab. */
  aiExpressionsDrawing?: boolean
  /** Resolves once the server draws none of the expressions any more. */
  awaitAiExpressions?: () => Promise<void>
  /** Draws the gesture arms the portrait lacks (all again with `again`); absent where they are not offered (full body). */
  onGenerateArmVariants?: (again?: boolean) => Promise<void>
  /** How many gesture arms the current portrait has already. */
  armVariantsReady?: number
  /** Gesture arms being drawn on the server, begun before a reload or in another tab. */
  armVariantsDrawing?: boolean
  /** Resolves once the server draws none of the gesture arms any more. */
  awaitArmVariants?: () => Promise<void>
}

/**
 * A PSD's way into the rig: split remotely or uploaded, preflighted, then
 * committed. The state outlives tab switches, so it lives above the panel.
 */
export function useRigImport({
  sourceMasterAssetId,
  sourceGenerationFingerprint,
  seeThroughTokenConfigured,
  onSaveSeeThroughToken,
  onDecomposeRigPsd,
  onResumeRigPsd,
  onDecomposePlainPsd,
  bodyKeysOffered = false,
  onPreflightRigPsd,
  onCommitRigPsd,
  onGenerateAiExpressions,
  aiExpressionsReady = false,
  aiExpressionsDrawing = false,
  awaitAiExpressions,
  onGenerateArmVariants,
  armVariantsReady = 0,
  armVariantsDrawing = false,
  awaitArmVariants,
}: RigImportSource) {
  const { t } = useI18n()
  const labels = t.merope
  const seeThroughErrors = {
    see_through_token_required: labels.motionSeeThroughTokenRequired,
    see_through_busy: labels.motionSeeThroughBusy,
    see_through_auth_failed: labels.motionSeeThroughAuthFailed,
    see_through_quota_unavailable: labels.motionSeeThroughQuota,
    see_through_timeout: labels.motionSeeThroughTimeout,
    see_through_upstream_failed: labels.motionSeeThroughUpstream,
    see_through_invalid_input: labels.motionSeeThroughUpstream,
    see_through_space_waking: t.errors.byCode.see_through_space_waking,
    see_through_space_unavailable: t.errors.byCode.see_through_space_unavailable,
  }
  const [stage, setStage] = useState<RigAssetCompileEvent | null>(null)
  // Where a decomposition with keyed turns stands, while it runs.
  const [decomposeStatus, setDecomposeStatus] = useState<TurnKeyformsStatus | null>(null)
  const [steps, setSteps] = useState<RigImportStepState>({})
  const [result, setResult] = useState<RigImportSummary | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [preflight, setPreflight] = useState<RigAssetPreflight | null>(null)
  const [tokenDraft, setTokenDraft] = useState('')
  const [tokenError, setTokenError] = useState<string | undefined>()
  const [operation, setOperation] = useState<
    'decompose' | 'manual' | 'commit' | null
  >(null)
  const importing = operation !== null
  const [aiExpressionsBusy, setAiExpressionsBusy] = useState(false)
  // A resumed split reads it once the expressions drawn meanwhile are in.
  const aiExpressionsReadyRef = useRef(aiExpressionsReady)
  aiExpressionsReadyRef.current = aiExpressionsReady
  const [aiExpressionsError, setAiExpressionsError] = useState<string | null>(
    null,
  )
  const [armVariantsBusy, setArmVariantsBusy] = useState(false)
  const [armVariantsError, setArmVariantsError] = useState<string | null>(null)
  const [mode, setMode] = useState<RigMode>('plain')
  // What the enhanced rig adds; the body's keys and the expressions only where they are offered.
  const [enhanceTurn, setEnhanceTurn] = useState(true)
  const [enhanceBody, setEnhanceBody] = useState(true)
  const [enhanceExpressions, setEnhanceExpressions] = useState(Boolean(onGenerateAiExpressions))
  const expressionsOffered = Boolean(onGenerateAiExpressions)
  const [enhanceArms, setEnhanceArms] = useState(Boolean(onGenerateArmVariants))
  const armsOffered = Boolean(onGenerateArmVariants)
  const enhanceParts: EnhanceParts = { head: enhanceTurn, body: bodyKeysOffered && enhanceBody }
  const enhanceChosen = enhanceParts.head || enhanceParts.body || (expressionsOffered && enhanceExpressions)
    || (armsOffered && enhanceArms)
  // The PSD last preflighted here, and whether with the redrawn expressions
  // and the gesture arms, so fresh ones can be tried on it at once.
  const lastRigPsdRef = useRef<File | null>(null)
  const lastWithExpressionsRef = useRef(false)
  const lastWithArmsRef = useRef(false)
  const importAbortRef = useRef<AbortController | null>(null)
  // The preview to send again: what the Worker made is kept when only that request failed.
  const previewRetryRef = useRef<RigPreviewRetry | null>(null)
  const [previewRetryable, setPreviewRetryable] = useState(false)
  const failRigImport = (message: string, reason?: unknown) => {
    setError(message)
    previewRetryRef.current = rigPreviewRetryOf(reason) ?? null
    setPreviewRetryable(previewRetryRef.current !== null)
    showStickyToast({
      message,
      type: 'error',
      replaceKey: 'merope-rig-import',
    })
  }
  const clearError = () => {
    setError(null)
    previewRetryRef.current = null
    setPreviewRetryable(false)
  }

  useEffect(() => {
    importAbortRef.current?.abort()
    importAbortRef.current = null
    setOperation(null)
    setPreflight(null)
    setStage(null)
    setSteps({})
    setResult(null)
    clearError()
    return () => { importAbortRef.current?.abort() }
  }, [sourceGenerationFingerprint, sourceMasterAssetId])

  const saveToken = async (token: string) => {
    if (!token || token.includes('•') || token.includes('*')) return
    setTokenError(undefined)
    try {
      await onSaveSeeThroughToken(token)
      setTokenDraft('')
    } catch (reason) {
      setTokenError(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.motionSeeThroughTokenFailed,
            labels.motionSeeThroughTimeout,
          ),
          labels.motionSeeThroughTokenFailed,
        ),
      )
      throw reason
    }
  }

  const editToken = (value: string) => {
    setTokenDraft(value)
    setTokenError(undefined)
  }

  const recordStage = (event: RigAssetCompileEvent) => {
    setStage(event)
    setSteps((current) => ({
      ...current,
      [event.stage]: event.status,
    }))
  }

  /**
   * Runs a preflight as the operation that can be cancelled: a PSD's from its
   * start, or only its failed preview again. A preview sent again keeps what
   * the Worker made until it has an answer, so cancelling it loses nothing.
   */
  const runPreflight = async (
    start: (onStage: (event: RigAssetCompileEvent) => void, signal: AbortSignal) => Promise<RigAssetPreflight>,
    fromStart: boolean,
  ) => {
    const controller = new AbortController()
    importAbortRef.current = controller
    setOperation('manual')
    if (fromStart) {
      setStage(null)
      setSteps({})
      clearError()
    }
    setResult(null)
    try {
      const imported = await start(
        (event) => {
          if (!controller.signal.aborted) recordStage(event)
        },
        controller.signal,
      )
      controller.signal.throwIfAborted()
      clearError()
      setPreflight(imported)
      setResult({
        partCount: imported.partCount,
        score: imported.report.score,
        activated: false,
      })
    } catch (reason) {
      if (controller.signal.aborted) return
      failRigImport(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.rigImportFailed,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.rigImportFailed,
        ),
        reason,
      )
    } finally {
      if (importAbortRef.current === controller && !controller.signal.aborted) {
        importAbortRef.current = null
        setOperation(null)
      }
    }
  }

  const preflightPsd = async (file: File, aiExpressions = false, armVariants = false) => {
    if (importing || !sourceMasterAssetId) return
    lastRigPsdRef.current = file
    lastWithExpressionsRef.current = aiExpressions
    lastWithArmsRef.current = armVariants
    // That a drawing failed is said beside the preflight that asked for it, not beside one that did not.
    if (!aiExpressions) setAiExpressionsError(null)
    if (!armVariants) setArmVariantsError(null)
    await runPreflight(
      (onStage, signal) => onPreflightRigPsd(file, onStage, signal, { aiExpressions, armVariants }),
      true,
    )
  }

  /** Sends a failed preview again with what the Worker already made: the PSD is not read and packed a second time. */
  const retryPreview = async () => {
    const retry = previewRetryRef.current
    if (importing || !retry) return
    await runPreflight(retry, false)
  }

  /** Preflights what a split made, with the redrawn expressions and gesture arms when they were chosen. */
  const preflightSplit = async (file: File, expressions: boolean, arms: boolean, controller: AbortController) => {
    setDecomposeStatus(null)
    controller.signal.throwIfAborted()
    lastRigPsdRef.current = file
    lastWithExpressionsRef.current = expressions
    lastWithArmsRef.current = arms
    const imported = await onPreflightRigPsd(file,
      (event) => {
        if (!controller.signal.aborted) recordStage(event)
      },
      controller.signal,
      { aiExpressions: expressions, armVariants: arms },
    )
    controller.signal.throwIfAborted()
    setPreflight(imported)
    setResult({
      partCount: imported.partCount,
      score: imported.report.score,
      activated: false,
    })
  }

  const failSplit = (reason: unknown) => {
    failRigImport(
      userFacingError(
        generationFailureMessage(
          reason,
          labels.motionSeeThroughUpstream,
          labels.motionSeeThroughTimeout,
          seeThroughErrors,
        ),
        labels.motionSeeThroughUpstream,
      ),
      reason,
    )
  }

  // A split the server has for this portrait that this page did not start
  // (still running, or ended with no page there) shows its progress again and
  // is preflighted when done, as if it had been started here; one that failed
  // shows why. It never redraws expressions, it only uses ones already made.
  // What it was asked for is the job's to say: the boxes take it, and so does
  // what its result is preflighted with.
  useEffect(() => {
    if (!onResumeRigPsd || !sourceMasterAssetId) return
    const controller = new AbortController()
    let picked = false
    let asked: TurnKeyformsAsked | undefined
    const pick = () => {
      if (picked || controller.signal.aborted) return
      picked = true
      importAbortRef.current = controller
      setMode('enhanced')
      setOperation('decompose')
      if (!asked) return
      const boxes = boxesOfPickedUp(asked)
      if (boxes.turn !== null) setEnhanceTurn(boxes.turn)
      if (boxes.body !== null) setEnhanceBody(boxes.body)
      if (boxes.expressions !== null) setEnhanceExpressions(boxes.expressions)
      if (boxes.arms !== null) setEnhanceArms(boxes.arms)
    }
    void (async () => {
      try {
        const file = await onResumeRigPsd((status) => {
          if (controller.signal.aborted) return
          asked = status.asked ?? asked
          pick()
          setDecomposeStatus(status)
        }, controller.signal)
        if (!file || controller.signal.aborted) return
        pick()
        // Expressions and gesture arms still being drawn (begun before the reload) go in once done.
        await Promise.all([awaitAiExpressions?.(), awaitArmVariants?.()])
        if (controller.signal.aborted) return
        const beside = besideOfPickedUp(asked, { expressions: enhanceExpressions, arms: enhanceArms })
        await preflightSplit(
          file,
          expressionsOffered && beside.expressions && aiExpressionsReadyRef.current,
          armsOffered && beside.arms,
          controller,
        )
      } catch (reason) {
        if (controller.signal.aborted || !picked) return
        failSplit(reason)
      } finally {
        if (picked && importAbortRef.current === controller && !controller.signal.aborted) {
          importAbortRef.current = null
          setOperation(null)
        }
      }
    })()
    // Asked once per portrait: the callbacks are new each render.
    return () => controller.abort()
  }, [sourceGenerationFingerprint, sourceMasterAssetId])

  /**
   * Splits the portrait and preflights it, enhanced: fits the keys chosen (or
   * refits a kept job) and redraws the expressions as chosen, the two at
   * once, then uses what was made. Only the expressions: one plain split.
   */
  const decomposePsd = async (fromArchive?: TurnArchiveUse) => {
    // A kept job loaded as it was needs nothing of the Space.
    if (importing || !sourceMasterAssetId || (!seeThroughTokenConfigured && fromArchive?.refit !== false)) {
      return
    }
    const enhanced = mode === 'enhanced' || fromArchive !== undefined
    const turn = enhanced && (enhanceTurn || fromArchive !== undefined)
    const body = enhanced && enhanceParts.body && fromArchive === undefined
    const expressions = enhanced && expressionsOffered && enhanceExpressions
    const arms = enhanced && armsOffered && enhanceArms
    if (!enhanced || (!turn && !body && !expressions && !arms)) return
    const controller = new AbortController()
    importAbortRef.current = controller
    setOperation('decompose')
    setDecomposeStatus(null)
    setStage(null)
    setSteps({})
    setResult(null)
    clearError()
    try {
      const beside = { expressions, arms }
      const split = turn || body || !onDecomposePlainPsd
        ? onDecomposeRigPsd(
            (status) => { if (!controller.signal.aborted) setDecomposeStatus(status) },
            controller.signal,
            fromArchive,
            { head: turn, body },
            beside,
          )
        : onDecomposePlainPsd(
            controller.signal,
            (status) => { if (!controller.signal.aborted) setDecomposeStatus(status) },
            beside,
          )
      // Redrawn once per portrait; kept ones are used as they are. One that
      // could not be drawn does not throw the split away either: the rig goes
      // in with the ones it has, and the page says the drawing failed.
      setAiExpressionsError(null)
      const redraw = optionalDrawing(
        expressions && !aiExpressionsReady ? onGenerateAiExpressions?.() : undefined,
        (reason) => setAiExpressionsError(userFacingError(reason, labels.aiExpressionsFailed)),
      )
      // Only the gestures this portrait lacks are drawn. None drawn does not
      // throw the split away: the rig goes in with the ones it has.
      setArmVariantsError(null)
      const gestures = optionalDrawing(
        arms ? onGenerateArmVariants?.() : undefined,
        (reason) => setArmVariantsError(userFacingError(reason, labels.armVariantsFailed)),
      )
      const [file] = await Promise.all([split, redraw, gestures])
      await preflightSplit(file, expressions, arms, controller)
    } catch (reason) {
      if (controller.signal.aborted) return
      failSplit(reason)
    } finally {
      if (importAbortRef.current === controller && !controller.signal.aborted) {
        importAbortRef.current = null
        setOperation(null)
      }
    }
  }

  /** Stop an upload's preflight; the PSD stays unimported. */
  const cancelPreflight = () => {
    importAbortRef.current?.abort()
    importAbortRef.current = null
    setOperation(null)
    setStage(null)
    // A preview being sent again when cancelled is back where it failed, still there to send.
    if (previewRetryRef.current) setSteps((current) => ({ ...current, 'compile-preview': 'failed' }))
    else setSteps({})
  }

  const generateAiExpressions = async () => {
    if (aiExpressionsBusy || !onGenerateAiExpressions) return
    setAiExpressionsBusy(true)
    setAiExpressionsError(null)
    try {
      await onGenerateAiExpressions()
      // Tried at once on the PSD last preflighted with expressions.
      const file = lastRigPsdRef.current
      if (file && lastWithExpressionsRef.current && !importing) void preflightPsd(file, true, lastWithArmsRef.current)
    } catch (reason) {
      setAiExpressionsError(userFacingError(reason, labels.aiExpressionsFailed))
    } finally {
      setAiExpressionsBusy(false)
    }
  }

  const generateArmVariants = async () => {
    if (armVariantsBusy || !onGenerateArmVariants) return
    setArmVariantsBusy(true)
    setArmVariantsError(null)
    try {
      await onGenerateArmVariants(true)
      // Tried at once on the PSD last preflighted with gesture arms.
      const file = lastRigPsdRef.current
      if (file && lastWithArmsRef.current && !importing) void preflightPsd(file, lastWithExpressionsRef.current, true)
    } catch (reason) {
      setArmVariantsError(userFacingError(reason, labels.armVariantsFailed))
    } finally {
      setArmVariantsBusy(false)
    }
  }

  const commitPsd = async () => {
    if (importing || !preflight) return
    setOperation('commit')
    setStage(null)
    clearError()
    try {
      const imported = await onCommitRigPsd(preflight, recordStage)
      setResult({
        partCount: imported.partCount,
        score: imported.score,
        activated: true,
      })
      setPreflight(null)
    } catch (reason) {
      failRigImport(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.rigCommitFailed,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.rigCommitFailed,
        ),
      )
    } finally {
      setOperation(null)
    }
  }

  return {
    sourceMasterAssetId,
    mode,
    setMode,
    enhanceTurn,
    setEnhanceTurn,
    enhanceBody,
    setEnhanceBody,
    bodyKeysOffered,
    enhanceParts,
    enhanceChosen,
    enhanceExpressions,
    setEnhanceExpressions,
    expressionsOffered,
    stage,
    decomposeStatus,
    steps,
    result,
    error,
    preflight,
    operation,
    importing,
    tokenDraft,
    tokenError,
    editToken,
    saveToken,
    preflightPsd,
    decomposePsd,
    cancelPreflight,
    previewRetryable,
    retryPreview,
    commitPsd,
    aiExpressionsBusy: aiExpressionsBusy || aiExpressionsDrawing,
    aiExpressionsError,
    generateAiExpressions,
    enhanceArms,
    setEnhanceArms,
    armsOffered,
    armVariantsReady,
    armVariantsBusy: armVariantsBusy || armVariantsDrawing,
    armVariantsError,
    generateArmVariants,
  }
}

export type RigImport = ReturnType<typeof useRigImport>
