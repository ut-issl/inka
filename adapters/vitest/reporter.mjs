import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import ts from 'typescript';

const read = (path) => JSON.parse(readFileSync(path, 'utf8'));
const write = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
const hash = (bytes) => 'sha256:' + createHash('sha256').update(bytes).digest('hex');
const exact = (value, keys) => Object.keys(value).sort().join() === keys.sort().join();

export function extract(file, source, test) {
  if (!test.location || test.options.each || test.options.fails || test.options.retry || test.options.repeats) {
    throw new Error('inka requires located, static tests without each/fails/retry/repeats');
  }
  const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
  if (ast.parseDiagnostics.length) throw new Error('cannot parse selected TypeScript source');
  const offset = ast.getPositionOfLineAndCharacter(test.location.line - 1, test.location.column - 1);
  const matches = [];
  function visit(node) {
    if (ts.isCallExpression(node) && node.getStart(ast) <= offset && offset < node.end) {
      let root = node.expression;
      while (ts.isPropertyAccessExpression(root)) root = root.expression;
      const name = node.arguments[0];
      if (ts.isIdentifier(root) && ['test', 'it'].includes(root.text)
          && name && ts.isStringLiteralLike(name) && name.text === test.name
          && node.arguments.some((arg) => ts.isArrowFunction(arg) || ts.isFunctionExpression(arg))) {
        matches.push(node);
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(ast);
  if (matches.length !== 1) throw new Error('ambiguous or unsupported test declaration');
  return matches[0].getText(ast);
}

/** Captures source before collection; the source workflow still owns execution. */
export default class InkaReporter {
  constructor({ plan, out = '.vitest/inka', environment = 'default', inka = process.env.INKA_BIN || 'inka' }) {
    this.path = resolve(plan);
    this.plan = read(this.path);
    if (!exact(this.plan, ['v', 'source', 'execution', 'targets']) || this.plan.v !== 1
        || !Array.isArray(this.plan.targets) || !this.plan.targets.length || !environment.trim()) {
      throw new Error('invalid inka Vitest plan');
    }
    this.out = resolve(out);
    mkdirSync(dirname(this.out), { recursive: true });
    mkdirSync(this.out); // Never reuse a previous attempt's output.
    this.environment = environment;
    this.inka = inka;
    this.snapshots = new Map();
    this.prepared = new Map();
    this.failed = false;
    const ids = new Set();
    for (const spec of this.plan.targets) {
      if (!exact(spec, ['id', 'file', 'selector']) || ids.has(spec.id)) throw new Error('invalid or duplicate target');
      ids.add(spec.id);
      const file = resolve(dirname(this.path), spec.file);
      this.snapshots.set(file, readFileSync(file));
    }
  }

  onInit(vitest) {
    if (vitest.config.watch) throw new Error('inka evidence requires vitest run, not watch mode');
  }

  onTestModuleCollected(module) {
    try {
      const file = resolve(module.moduleId);
      const original = this.snapshots.get(file);
      if (!original) return;
      if (module.errors().length) throw new Error('selected module failed collection');
      for (const [index, spec] of this.plan.targets.entries()) {
        if (resolve(dirname(this.path), spec.file) !== file) continue;
        if (this.prepared.has(spec.id)) throw new Error('multiple projects or collections for the same target');
        const cases = [...module.children.allTests()].filter((test) => test.fullName === spec.selector);
        if (cases.length !== 1) throw new Error(`ambiguous or missing Vitest selector: ${spec.selector}`);
        const test = cases[0];
        const contract = resolve(this.out, `contract-${index}.txt`);
        writeFileSync(contract, extract(file, original.toString('utf8'), test));
        const required = [`${this.environment}/${spec.file}::${test.fullName}`];
        const plan = { v: 1, source: this.plan.source, execution: this.plan.execution,
          guards: [{ path: file, sha256: hash(original) }],
          targets: [{ id: spec.id, file: contract, kind: 'function', profile: 'vitest-source:v1', required }] };
        const planPath = resolve(this.out, `plan-${index}.json`);
        const receiptPath = resolve(this.out, `receipt-${index}.json`);
        write(planPath, plan);
        execFileSync(this.inka, ['ci-export', 'prepare', '--plan', planPath, '--out', receiptPath]);
        this.prepared.set(spec.id, { receipt: read(receiptPath), test });
      }
    } catch (error) {
      this.failed = true;
      throw error;
    }
  }

  onTestRunEnd(_modules, errors, reason) {
    if (this.failed || this.prepared.size !== this.plan.targets.length) {
      throw new Error('inka cannot export incomplete or invalid test collection');
    }
    const receipt = { v: 1, bundle: { v: 2, source: this.plan.source, execution: this.plan.execution, targets: [] }, files: [] };
    const results = { v: 1, source: this.plan.source, execution: this.plan.execution, targets: [] };
    for (const spec of this.plan.targets) {
      const entry = this.prepared.get(spec.id);
      const target = entry.receipt.bundle.targets[0];
      receipt.bundle.targets.push(target);
      receipt.files.push(...entry.receipt.files);
      let outcome = { passed: 'pass', failed: 'fail', skipped: 'skip' }[entry.test.result().state];
      for (let parent = entry.test.parent; parent; parent = parent.parent) {
        if (parent.errors().length) outcome = 'fail';
      }
      if (reason === 'interrupted' || errors.length) outcome = 'cancelled';
      results.targets.push({ id: target.id, interface_hash: target.interface_hash,
        results: outcome ? [{ case: target.required[0], result: outcome }] : [] });
    }
    write(resolve(this.out, 'receipt.json'), receipt);
    write(resolve(this.out, 'results.json'), results);
  }
}
