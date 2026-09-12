import type { LastFailedUpdate, UpdateMode } from '../../../services/updaterApi'

/**
 * Only a structured preflight decision can offer this retry. Never infer it
 * from error text, channel or a Docker Hub discovery source.
 */
export function tagInstallRetryOptions(failed: LastFailedUpdate) {
  if (!failed.confirmation_required || !failed.to_version
    || !['dockerhub_tag', 'dockerhub_commit'].includes(failed.trust?.trust_path ?? '')) { return null
}
  const mode: UpdateMode = failed.trust?.trust_path === 'dockerhub_tag' ? 'release' : 'commit'
  const flags = failed.risk_flags
  return {
    target: failed.to_version,
    opts: {
      mode,
      allowTagInstall: true,
      allowRisk: false,
      allowDowngrade: !!flags?.allow_downgrade,
      allowDiverged: !!flags?.allow_diverged,
      allowUnknown: !!flags?.allow_unknown,
      allowIrreversible: !!flags?.allow_irreversible,
    },
  }
}
