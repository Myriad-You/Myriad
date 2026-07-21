export interface BuildInfo {
  version: string
  commitSha: string | null
  commitUrl: string | null
}

const REPOSITORY_URL = 'https://github.com/Myriad-You/Myriad'

function validCommit(value: string | null | undefined): string | null {
  const commit = value?.trim().toLowerCase() ?? ''
  return /^[0-9a-f]{7,40}$/.test(commit) ? commit : null
}

export function getBuildInfo(): BuildInfo {
  const fallbackVersion = `v${__APP_VERSION__ || '0.3.10'}`
  if (typeof document === 'undefined') {
    return { version: fallbackVersion, commitSha: null, commitUrl: null }
  }

  const version =
    document
      .querySelector('meta[name="myriad-version"]')
      ?.getAttribute('content')
      ?.trim() || fallbackVersion
  const embeddedCommit = document
    .querySelector('meta[name="myriad-commit"]')
    ?.getAttribute('content')
  const versionCommit = version.startsWith('dev-') ? version.slice(4) : null
  const commitSha = validCommit(embeddedCommit) ?? validCommit(versionCommit)

  return {
    version,
    commitSha,
    commitUrl: commitSha ? `${REPOSITORY_URL}/commit/${commitSha}` : null,
  }
}
