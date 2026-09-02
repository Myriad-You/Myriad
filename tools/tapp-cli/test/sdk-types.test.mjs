import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import ts from 'typescript'
import { generateTappSdkDts } from '../scripts/sdk-dts.mjs'

it('generates typed reference-image requests and rejects malformed inputs', async () => {
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
      import type { TappSdk } from './sdk';
      declare const sdk: TappSdk;
      sdk.ai.tasks.create({ version: 2, operation: 'image', input: {
        prompt: 'draw', width: '768px', height: 1024,
        referenceImages: ['data:image/png;base64,AA==', '/api/brew/image-cache/aa/a.png'],
      }, output: { format: 'image' } });
      sdk.ai.tasks.create({ version: 2, operation: 'image', input: 'draw' });
      sdk.ai.tasks.create({ version: 2, operation: 'generate', input: { prompt: 'hello' } });
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
