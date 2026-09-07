import assert from 'node:assert/strict'
import test from 'node:test'
import { parseGithubRepoInput, parseGithubRepoList } from './githubRepo'

test('parseGithubRepoInput accepts owner/repo and URLs', () => {
  assert.deepEqual(parseGithubRepoInput('facebook/react'), {
    owner: 'facebook',
    repo: 'react',
  })
  assert.deepEqual(
    parseGithubRepoInput('https://github.com/vercel/next.js'),
    { owner: 'vercel', repo: 'next.js' },
  )
  assert.equal(parseGithubRepoInput('not a repo'), null)
})

test('parseGithubRepoList de-dupes and caps', () => {
  const list = parseGithubRepoList(
    'facebook/react\nhttps://github.com/facebook/react\nvercel/next.js\n',
  )
  assert.deepEqual(list, [
    { owner: 'facebook', repo: 'react' },
    { owner: 'vercel', repo: 'next.js' },
  ])
})
