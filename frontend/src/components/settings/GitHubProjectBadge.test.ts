import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('GitHubProjectBadge wiring', () => {
  it('puts both repo badges under the layered-rig tabs, and on Ollama docs', () => {
    const workbench = readFileSync(
      new URL(
        '../../features/merope/anime25drig/Anime25DWorkbench.tsx',
        import.meta.url,
      ),
      'utf8',
    )
    assert.match(workbench, /GitHubProjectBadge/)
    assert.match(workbench, /ANIME25D_PROJECT_URL/)
    assert.match(workbench, /ANIME25D_PROJECT_NAME/)
    assert.match(workbench, /SEE_THROUGH_PROJECT_URL/)
    assert.match(workbench, /SEE_THROUGH_PROJECT_NAME/)
    assert.match(
      workbench,
      /merope-character-home__credit[\s\S]*SEE_THROUGH_PROJECT_URL[\s\S]*ANIME25D_PROJECT_URL[\s\S]*PERSONA_UPSTREAM_THANKS/,
    )
    assert.doesNotMatch(
      workbench,
      /href="https:\/\/github\.com\/852wa\/Anime2\.5DRig"/,
    )

    const vendors = readFileSync(
      new URL('../config/AiVendorSources.tsx', import.meta.url),
      'utf8',
    )
    assert.match(vendors, /GitHubProjectBadge/)
    assert.match(vendors, /isGithubRepoUrl\(preset\.docs_url\)/)
  })
})
