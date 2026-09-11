import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, symlinkSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

const adapter = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const inka = process.env.INKA_BIN || resolve(adapter, '../../target/debug/inka');
const read = (path) => JSON.parse(readFileSync(path, 'utf8'));

function run(t, source, selectors) {
  const dir = mkdtempSync(resolve(tmpdir(), 'inka-vitest-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  symlinkSync(resolve(adapter, 'node_modules'), resolve(dir, 'node_modules'), 'dir');
  writeFileSync(resolve(dir, 'sample.test.ts'), source);
  writeFileSync(resolve(dir, 'selection.json'), JSON.stringify({ v: 1,
    source: { id: 'github:example/vitest', revision: 'snapshot' }, execution: { id: 'run-1', attempt: 1 },
    targets: selectors.map((selector, index) => ({ id: `contract-${index}`, file: 'sample.test.ts', selector })) }));
  writeFileSync(resolve(dir, 'vitest.config.mjs'), `
    import InkaReporter from ${JSON.stringify(pathToFileURL(resolve(adapter, 'reporter.mjs')).href)};
    export default { test: { include: ['sample.test.ts'], includeTaskLocation: true,
      reporters: ['default', new InkaReporter({plan:'selection.json', out:'output', environment:'linux', inka:${JSON.stringify(inka)}})] } };
  `);
  const execution = spawnSync(process.execPath, [resolve(adapter, 'node_modules/vitest/vitest.mjs'), 'run'],
    { cwd: dir, encoding: 'utf8', timeout: 30000 });
  const finish = () => spawnSync(inka, ['ci-export', 'finish', '--receipt', 'output/receipt.json',
    '--results', 'output/results.json', '--out', 'bundle.json'], { cwd: dir, encoding: 'utf8' });
  return { dir, execution, finish, bundle: () => read(resolve(dir, 'bundle.json')) };
}

test('extracts TypeScript declaration and passes with completed execution', (t) => {
  const result = run(t, `import {test, expect, describe} from 'vitest';
    const helper = () => 2;
    describe('suite', () => { test('typed', () => { const n: number = helper(); expect(n).toBe(2); }); });`, ['suite > typed']);
  assert.equal(result.execution.status, 0, result.execution.stdout + result.execution.stderr);
  assert.equal(result.finish().status, 0);
  const target = result.bundle().targets[0];
  assert.equal(target.kind, 'function');
  assert.match(target.content, /const n: number/);
  assert.doesNotMatch(target.content, /const helper|describe/);
  assert.deepEqual(target.results, [{ case: 'linux/sample.test.ts::suite > typed', result: 'pass' }]);
});

test('failure, skip and teardown failure cannot establish evidence', (t) => {
  const result = run(t, `import {test, expect, afterAll, describe} from 'vitest';
    test('failed', () => expect(1).toBe(2));
    test.skip('skipped', () => {});
    describe('hooks', () => { afterAll(() => { throw Error('teardown'); }); test('passed', () => {}); });`,
  ['failed', 'skipped', 'hooks > passed']);
  assert.equal(result.execution.status, 1);
  assert.equal(result.finish().status, 1);
  assert.deepEqual(result.bundle().targets.map((target) => target.results[0].result), ['fail', 'skip', 'fail']);
});

test('source modification during a test invalidates finish', (t) => {
  const result = run(t, `import {test} from 'vitest'; import {appendFileSync} from 'node:fs';
    test('mutates', () => { appendFileSync('sample.test.ts', '\\n// changed'); });`, ['mutates']);
  assert.equal(result.execution.status, 0, result.execution.stderr);
  const finished = result.finish();
  assert.equal(finished.status, 1);
  assert.match(finished.stderr, /changed/);
  assert.equal(existsSync(resolve(result.dir, 'bundle.json')), false);
});

test('import-time modification and ambiguous selectors cannot export', (t) => {
  for (const source of [
    `import {test} from 'vitest'; import {appendFileSync} from 'node:fs'; appendFileSync('sample.test.ts', '\\n// changed'); test('same', () => {});`,
    `import {test} from 'vitest'; test('same', () => {}); test('same', () => {});`,
    `import {test} from 'vitest'; test.each([1])('same', () => {});`,
  ]) {
    const result = run(t, source, ['same']);
    assert.notEqual(result.execution.status, 0, result.execution.stdout);
    assert.equal(existsSync(resolve(result.dir, 'output/receipt.json')), false);
  }
});
