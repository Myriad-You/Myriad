import { readdirSync, readFileSync } from 'node:fs'

const DIR = new URL('./', import.meta.url)

/** One workbench file's source. */
export function workbenchSource(name: string): string {
  return readFileSync(new URL(name, DIR), 'utf8')
}

/** Workbench modules together, for checks that hold across the split. */
export function workbenchSources(except: readonly string[] = []): string {
  return readdirSync(DIR)
    .filter((name) => /\.tsx?$/.test(name) && !/\.test(-support)?\.tsx?$/.test(name))
    .filter((name) => !except.includes(name))
    .sort()
    .map(workbenchSource)
    .join('\n')
}
