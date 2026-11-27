/**
 * Starts the `inka serve` the recording tests run against: one that records
 * decisions, on a throwaway copy of the aocs fixture, so that nothing a test
 * decides is ever written into the repository.
 */

import { spawn } from 'node:child_process'
import { cpSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const [port, approver] = process.argv.slice(2)
const fixture = fileURLToPath(new URL('../../tests/fixtures/aocs', import.meta.url))
const project = mkdtempSync(join(tmpdir(), 'inka-e2e-'))
cpSync(fixture, project, { recursive: true })

const server = spawn(
  'cargo',
  ['run', '--locked', '-p', 'inka-cli', '--', 'serve', project, '--port', port, '--as', approver],
  { stdio: 'inherit' },
)

const stop = () => {
  server.kill()
  rmSync(project, { recursive: true, force: true })
}
process.on('SIGTERM', stop)
process.on('SIGINT', stop)
server.on('exit', (code) => {
  rmSync(project, { recursive: true, force: true })
  process.exit(code ?? 0)
})
