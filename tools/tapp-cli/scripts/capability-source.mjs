export function parseCapabilitySource(source) {
  const deniedBlock = source.match(
    /export const HEADLESS_DENIED_ACTIONS\s*=\s*\[([\s\S]*?)\]\s*as const/,
  )
  if (!deniedBlock) throw new Error('Unable to locate HEADLESS_DENIED_ACTIONS')

  const headlessDeniedActions = []
  for (const match of deniedBlock[1].matchAll(/'([^']+)'/g)) {
    headlessDeniedActions.push(match[1])
  }

  if (headlessDeniedActions.length < 10) {
    throw new Error('Capability profile parse produced unexpectedly few denied actions')
  }

  const profilesBlock = source.match(
    /export type SandboxCapabilityProfile\s*=\s*([\s\S]*?)\n/,
  )
  const profiles = []
  if (profilesBlock) {
    for (const match of profilesBlock[1].matchAll(/'([^']+)'/g)) {
      profiles.push(match[1])
    }
  }
  if (profiles.length === 0) {
    profiles.push('page', 'widget', 'headless')
  }

  return {
    profiles,
    headlessDeniedActions,
  }
}
