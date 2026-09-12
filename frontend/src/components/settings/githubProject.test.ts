import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  fetchGithubStarCount,
  formatStarCount,
  GITHUB_STAR_TTL_MS,
  githubRepoUrl,
  isGithubRepoUrl,
  parseGithubRepoUrl,
  readStarCache,
  resetGithubStarMemoryForTests,
  writeStarCache,
} from './githubProject'

function memoryStorage(): Storage {
  const map = new Map<string, string>()
  return {
    get length() {
      return map.size
    },
    clear() {
      map.clear()
    },
    getItem(key) {
      return map.get(key) ?? null
    },
    key(index) {
      return Iterator.from(map.keys()).toArray()[index] ?? null
    },
    removeItem(key) {
      map.delete(key)
    },
    setItem(key, value) {
      map.set(key, value)
    },
  }
}

afterEach(() => {
  resetGithubStarMemoryForTests()
})

describe('parseGithubRepoUrl', () => {
  it('accepts owner/repo, including extra path and .git', () => {
    assert.deepEqual(
      parseGithubRepoUrl('https://github.com/852wa/Anime2.5DRig'),
      {
        owner: '852wa',
        repo: 'Anime2.5DRig',
      },
    )
    assert.deepEqual(
      parseGithubRepoUrl('https://www.github.com/ollama/ollama'),
      {
        owner: 'ollama',
        repo: 'ollama',
      },
    )
    assert.deepEqual(
      parseGithubRepoUrl('github.com/ollama/ollama/blob/main/README.md'),
      { owner: 'ollama', repo: 'ollama' },
    )
    assert.deepEqual(
      parseGithubRepoUrl('https://github.com/ollama/ollama.git'),
      {
        owner: 'ollama',
        repo: 'ollama',
      },
    )
  })

  it('rejects GitHub product pages and org roots', () => {
    assert.equal(parseGithubRepoUrl('https://github.com/'), null)
    assert.equal(
      parseGithubRepoUrl('https://github.com/settings/tokens'),
      null,
    )
    assert.equal(
      parseGithubRepoUrl('https://github.com/settings/developers'),
      null,
    )
    assert.equal(parseGithubRepoUrl('https://github.com/Myriad-You'), null)
    assert.equal(parseGithubRepoUrl('https://github.com/orgs/foo'), null)
    assert.equal(parseGithubRepoUrl('https://gitlab.com/foo/bar'), null)
    assert.equal(
      isGithubRepoUrl('https://github.com/settings/tokens'),
      false,
    )
    assert.equal(isGithubRepoUrl('https://github.com/ollama/ollama'), true)
  })
})

describe('formatStarCount', () => {
  it('uses GitHub-style compact counts', () => {
    assert.equal(formatStarCount(0), '0')
    assert.equal(formatStarCount(999), '999')
    assert.equal(formatStarCount(1000), '1k')
    assert.equal(formatStarCount(1200), '1.2k')
    assert.equal(formatStarCount(10500), '10.5k')
    assert.equal(formatStarCount(1_200_000), '1.2m')
  })
})

describe('github star cache', () => {
  it('keeps stars for 7 days and drops afterwards', () => {
    const storage = memoryStorage()
    writeStarCache('ollama', 'ollama', 150_000, 1_000, storage)
    assert.equal(readStarCache('ollama', 'ollama', 1_000, storage), 150_000)
    assert.equal(
      readStarCache(
        'ollama',
        'ollama',
        1_000 + GITHUB_STAR_TTL_MS - 1,
        storage,
      ),
      150_000,
    )
    resetGithubStarMemoryForTests()
    assert.equal(
      readStarCache(
        'ollama',
        'ollama',
        1_000 + GITHUB_STAR_TTL_MS,
        storage,
      ),
      null,
    )
  })

  it('returns cached stars without fetching', async () => {
    const storage = memoryStorage()
    writeStarCache('852wa', 'Anime2.5DRig', 42, 10, storage)
    let calls = 0
    const count = await fetchGithubStarCount('852wa', 'Anime2.5DRig', {
      now: 10,
      storage,
      fetchImpl: async () => {
        calls += 1
        return new Response('{}')
      },
    })
    assert.equal(count, 42)
    assert.equal(calls, 0)
    assert.equal(
      githubRepoUrl({ owner: '852wa', repo: 'Anime2.5DRig' }),
      'https://github.com/852wa/Anime2.5DRig',
    )
  })

  it('fetches and stores stars when cache is empty', async () => {
    const storage = memoryStorage()
    let requested = ''
    const count = await fetchGithubStarCount('ollama', 'ollama', {
      now: 50,
      storage,
      fetchImpl: async (input) => {
        requested = String(input)
        return new Response(JSON.stringify({ stars: 1234 }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      },
    })
    assert.equal(count, 1234)
    assert.match(requested, /\/api\/github\/repo\?/)
    assert.match(requested, /owner=ollama/)
    assert.match(requested, /repo=ollama/)
    resetGithubStarMemoryForTests()
    assert.equal(readStarCache('ollama', 'ollama', 50, storage), 1234)
  })
})
