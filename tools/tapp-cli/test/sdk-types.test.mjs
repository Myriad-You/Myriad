import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import ts from 'typescript'
import { generateTappSdkDts } from '../scripts/sdk-dts.mjs'

it('types AI task inputs and snapshots and rejects malformed inputs', async () => {
  const contract = JSON.parse(await readFile(new URL('../src/generated/contract.json', import.meta.url), 'utf8'))
  const generated = generateTappSdkDts({
    actions: contract.permissions.actions,
    headlessDeniedActions: contract.capabilities.headlessDeniedActions,
  })
  const shipped = await readFile(new URL('../src/generated/tapp-sdk.d.ts', import.meta.url), 'utf8')
  assert.equal(shipped, generated)
  const directory = await mkdtemp(join(tmpdir(), 'tapp-image-types-'))
  try {
    await writeFile(join(directory, 'sdk.d.ts'), shipped)
    await writeFile(join(directory, 'example.ts'), `
      import type { TappAITaskSnapshot, TappAIUsageSnapshot, TappSdk } from './sdk';
      declare const sdk: TappSdk;
      sdk.ai.tasks.create({ version: 2, operation: 'image', input: {
        prompt: 'draw', width: '768px', height: 1024,
        referenceImages: ['data:image/png;base64,AA==', '/api/brew/image-cache/aa/a.png'],
      }, output: { format: 'image' } });
      sdk.ai.tasks.create({ version: 2, operation: 'image', input: 'draw' });
      sdk.ai.tasks.create({ version: 2, operation: 'generate', input: { prompt: 'hello' } });
      sdk.ai.tasks.create({ version: 2, operation: 'generate', input: 'hello' });
      sdk.ai.tasks.create({ version: 2, operation: 'analyze', input: { data: { n: 1 }, instruction: 'trend' } });
      sdk.ai.tasks.create({ version: 2, operation: 'chat', input: { messages: [{ role: 'user', content: 'hi' }] } });
      sdk.ai.tasks.create({ version: 2, operation: 'search', input: { query: 'rss rust', searchType: 'general' }, output: { format: 'json' } });
      sdk.ai.tasks.create({ version: 2, operation: 'search', input: 'rss rust' });
      declare function expectSnapshot(value: TappAITaskSnapshot): void;
      declare function expectUsage(value: TappAIUsageSnapshot): void;
      sdk.ai.tasks.create({ version: 2, operation: 'generate', input: 'hello' }).then(expectSnapshot);
      sdk.ai.tasks.get('task-id').then(expectSnapshot);
      sdk.ai.tasks.usage().then(expectUsage);
      sdk.ai.tasks.cancel('task-id').then((cancelled) => {
        cancelled.success;
        cancelled.taskId;
      });
      // @ts-expect-error Generate object input requires prompt.
      sdk.ai.tasks.create({ version: 2, operation: 'generate', input: {} });
      // @ts-expect-error Analyze input requires data.
      sdk.ai.tasks.create({ version: 2, operation: 'analyze', input: { instruction: 'trend' } });
      // @ts-expect-error Analyze input must be an object with data.
      sdk.ai.tasks.create({ version: 2, operation: 'analyze', input: 'raw' });
      // @ts-expect-error Chat input requires messages.
      sdk.ai.tasks.create({ version: 2, operation: 'chat', input: { prompt: 'hi' } });
      // @ts-expect-error Chat role must be system, user, or assistant.
      sdk.ai.tasks.create({ version: 2, operation: 'chat', input: { messages: [{ role: 'tool', content: 'x' }] } });
      // @ts-expect-error Search input requires query.
      sdk.ai.tasks.create({ version: 2, operation: 'search', input: { searchType: 'general' } });
      // @ts-expect-error Reference images must be strings.
      sdk.ai.tasks.create({ version: 2, operation: 'image', input: { prompt: 'draw', referenceImages: [42] } });
      // @ts-expect-error Image input requires a prompt.
      sdk.ai.tasks.create({ version: 2, operation: 'image', input: { referenceImages: [] } });
    `)
    const program = ts.createProgram([join(directory, 'example.ts')], {
      strict: true, noEmit: true, target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.NodeNext, moduleResolution: ts.ModuleResolutionKind.NodeNext,
      types: [],
    })
    assert.deepEqual(ts.getPreEmitDiagnostics(program).map(diagnostic =>
      ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n')), [])
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

it('exposes global Tapp to referenced JavaScript without importing the SDK module', async () => {
  const shipped = await readFile(new URL('../src/generated/tapp-sdk.d.ts', import.meta.url), 'utf8')
  const directory = await mkdtemp(join(tmpdir(), 'tapp-global-'))
  try {
    await writeFile(join(directory, 'tapp-sdk.d.ts'), shipped)
    await writeFile(
      join(directory, 'page.js'),
      `/// <reference path="./tapp-sdk.d.ts" />
Tapp.lifecycle.onReady(async () => {
  await Tapp.storage.get('ready')
})
`,
    )
    const program = ts.createProgram([join(directory, 'page.js')], {
      allowJs: true,
      checkJs: true,
      noEmit: true,
      target: ts.ScriptTarget.ES2022,
      lib: ['lib.es2022.d.ts', 'lib.dom.d.ts'],
      types: [],
    })
    assert.deepEqual(
      ts.getPreEmitDiagnostics(program).map((diagnostic) =>
        ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
      ),
      [],
    )
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
