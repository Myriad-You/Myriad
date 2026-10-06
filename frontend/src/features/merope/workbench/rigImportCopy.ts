import type { TranslationKeys } from '../../../i18n'
import type { RigAssetCompileEvent } from '../assets/pipeline'
import type { AuthoredExpressionKind } from '../rig/authoredExpression'
import type { TurnKeyformsStatus } from '../turnKeyformsApi'

type Labels = TranslationKeys['merope']

export const RIG_IMPORT_STEP_ORDER: RigAssetCompileEvent['stage'][] = [
  'validate-source',
  'pack-atlas',
  'compile-preview',
  'analyze-capabilities',
  'persist-manifest',
]

export type RigImportStepStatus = RigAssetCompileEvent['status'] | 'pending'

export function rigImportStepCopy(
  labels: Labels,
  stage: RigAssetCompileEvent['stage'],
): { title: string; description: string } {
  if (stage === 'validate-source') {
    return {
      title: labels.rigPreflightStepValidate,
      description: labels.rigPreflightStepValidateDescription,
    }
  }
  if (stage === 'pack-atlas') {
    return {
      title: labels.rigPreflightStepPack,
      description: labels.rigPreflightStepPackDescription,
    }
  }
  if (stage === 'compile-preview') {
    return {
      title: labels.rigPreflightStepPreview,
      description: labels.rigPreflightStepPreviewDescription,
    }
  }
  if (stage === 'analyze-capabilities') {
    return {
      title: labels.rigPreflightStepAnalyze,
      description: labels.rigPreflightStepAnalyzeDescription,
    }
  }
  return {
    title: labels.rigPreflightStepActivate,
    description: labels.rigPreflightStepActivateDescription,
  }
}

export function rigImportStatusLabel(
  labels: Labels,
  status: RigImportStepStatus,
): string {
  if (status === 'started') return labels.rigPreflightStatusRunning
  if (status === 'completed') return labels.rigPreflightStatusCompleted
  if (status === 'failed') return labels.rigPreflightStatusFailed
  return labels.rigPreflightStatusPending
}

export function rigDiagnosticSeverityLabel(
  labels: Labels,
  severity: 'error' | 'warning' | 'info',
): string {
  if (severity === 'error') return labels.rigDiagnosticSeverityError
  if (severity === 'warning') return labels.rigDiagnosticSeverityWarning
  return labels.rigDiagnosticSeverityInfo
}

export function rigDiagnosticMessage(
  labels: Labels,
  code: string,
  fallback: string,
): string {
  if (code === 'missing-presentation-fallback') {
    return labels.rigDiagnosticMissingPresentationFallback
  }
  if (code === 'unknown-presentation-variant') {
    return labels.rigDiagnosticUnknownPresentationVariant
  }
  if (code === 'missing-head') return labels.rigDiagnosticMissingHead
  if (code === 'missing-body') return labels.rigDiagnosticMissingBody
  if (code === 'missing-mouth') return labels.rigDiagnosticMissingMouth
  if (code === 'missing-gaze') return labels.rigDiagnosticMissingGaze
  if (code === 'missing-facial-variants') {
    return labels.rigDiagnosticMissingFacialVariants
  }
  if (code === 'missing-secondary-motion') {
    return labels.rigDiagnosticMissingSecondaryMotion
  }
  if (code === 'missing-outfit-profile') {
    return labels.rigDiagnosticMissingOutfitProfile
  }
  if (code === 'missing-spatial-profile') {
    return labels.rigDiagnosticMissingSpatialProfile
  }
  if (code === 'rigid-part-deformation') {
    return labels.rigDiagnosticRigidPartDeformation
  }
  if (code === 'off-angle-portrait') return labels.rigDiagnosticOffAnglePortrait
  if (code === 'hole-under-eyes') return labels.rigDiagnosticHoleUnderEyes
  if (code === 'hole-under-mouth') return labels.rigDiagnosticHoleUnderMouth
  return fallback
}

export function aiExpressionLabel(
  labels: Labels,
  kind: AuthoredExpressionKind,
): string {
  if (kind === 'cry') return labels.aiExpressionCry
  if (kind === 'squeeze') return labels.aiExpressionSqueeze
  return labels.aiExpressionClose
}

/** What a decomposition with keyed turns is doing now. */
export function turnKeysProgress(labels: Labels, status: TurnKeyformsStatus): string {
  const counts = { done: String(status.done), total: String(status.total) }
  const template = status.stage === 'generating'
    ? labels.motionTurnKeysGenerating
    : status.stage === 'decomposing'
      ? labels.motionTurnKeysDecomposing
      : labels.motionTurnKeysFitting
  return template.replace(/\{(done|total)\}/g, (_, key: 'done' | 'total') => counts[key])
}
