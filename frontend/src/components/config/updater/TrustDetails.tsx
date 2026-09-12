import type { UpdateTrust } from '../../../services/updaterApi'
import type { U } from './helpers'

export function TrustDetails({ trust, u }: { trust?: UpdateTrust | null; u: U }) {
  if (!trust) return null
  const paths = {
    github_release: u.updaterTrustManifest,
    signed_commit: u.updaterTrustSignedCommit,
    dockerhub_tag: u.updaterTrustTag,
    dockerhub_commit: u.updaterTrustLegacyCommit,
  }
  const verification = trust.verification === 'verified'
    ? u.updaterTrustVerified
    : trust.verification === 'pending' ? u.updaterTrustPending : u.updaterTrustUnverified
  return (
    <div className="updater-trust">
      <p>{paths[trust.trust_path] ?? trust.trust_path} · {verification}</p>
      <details>
        <summary>{u.updaterTrustEvidence}</summary>
        <p><code>{trust.trust_path}</code> · <code>{trust.verification}</code></p>
        {trust.commit_sha && <p>Commit: <code>{trust.commit_sha}</code></p>}
        {(['backend', 'frontend'] as const).map(component => {
          const image = trust[component]
          return image && <p key={component}>{component}: <code>{image.ref}@{image.digest}</code></p>
        })}
      </details>
    </div>
  )
}
