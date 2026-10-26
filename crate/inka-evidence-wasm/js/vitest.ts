/**
 * Vitest reporter emitting inka test evidence v1.
 *
 * Configure it in the vitest config, which must also enable task
 * locations so contract bodies can be sliced from the source:
 *
 * ```ts
 * export default defineConfig({
 *   test: {
 *     includeTaskLocation: true,
 *     reporters: [
 *       ["default"],
 *       ["@ut-issl/inka-evidence/vitest", { output: "evidence.json" }],
 *     ],
 *   },
 * });
 * ```
 *
 * The declared set is the run's collected tests, never inferred from
 * results: an interrupted or bailed run leaves declared executions with a
 * missing outcome. Whenever the declared set would be unreliable — a test
 * file that failed to collect — nothing is emitted and the reason is
 * printed; the output path is removed when the run starts, so a leftover
 * file can never pass for this run's evidence.
 *
 * The test-id is `[project] <relative path> > <describe chain> > <name>`
 * (the project prefix only when the project is named), matching Vitest's
 * own display. Each `test.each` instance is its own test under its
 * formatted name. One execution per run, labeled by the `case` option
 * (default `default`).
 *
 * Bodies are the whole test call expression — declaration and callback,
 * for `test.each` the full chain including the table — sliced verbatim
 * from the original source at the task's reported location. The resolved
 * call is cross-checked against the task: a literal test name must match
 * (each patterns are matched placeholder-wise), and the tagged-template
 * `each` form is rejected outright because Vitest reports untrustworthy
 * locations for it. Anything the slicer cannot resolve or verify is an
 * explicit error, never a guess.
 */

import { readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";

import { parse, type ParserPlugin } from "@babel/parser";
import type { Reporter, TestModule, Vitest } from "vitest/node";

import { EvidenceBuilder } from "../pkg/inka_evidence_wasm.js";

/** Options passed in the vitest config's reporter tuple. */
export interface InkaEvidenceOptions {
  /** Where to write the evidence JSON, relative to the vitest root. */
  output: string;
  /** The case ID recorded for this run's execution configuration. */
  case?: string;
  /** Extra provenance entries for the evidence file. */
  provenance?: Record<string, string>;
}

const DEFAULT_CASE = "default";

/** A parsed AST node; only the shape the walker relies on is typed. */
interface AstNode {
  type: string;
  start: number | null;
  end: number | null;
  callee?: AstNode;
  [key: string]: unknown;
}

export default class InkaEvidenceReporter implements Reporter {
  private readonly options: InkaEvidenceOptions;
  private vitest: Vitest | undefined;

  constructor(options?: Partial<InkaEvidenceOptions>) {
    if (!options || typeof options.output !== "string" || options.output === "") {
      throw new TypeError(
        "inka-evidence: the reporter needs an `output` path option",
      );
    }
    this.options = { ...options, output: options.output };
  }

  onInit(vitest: Vitest): void {
    this.vitest = vitest;
    // A leftover file from an earlier run must never pass for this run's
    // evidence: after a suppressed emission the path stays absent.
    rmSync(this.outputPath(), { force: true });
    rmSync(`${this.outputPath()}.tmp`, { force: true });
  }

  onTestRunEnd(
    testModules: ReadonlyArray<TestModule>,
    unhandledErrors: ReadonlyArray<unknown>,
  ): void {
    const uncollected = testModules.filter(
      (module) => module.errors().length > 0,
    );
    if (uncollected.length > 0) {
      const names = uncollected.map((module) => module.moduleId).join(", ");
      this.announce(
        `collection errors in ${names}: the declared set is unreliable, ` +
          "no evidence was emitted",
      );
      return;
    }
    if (unhandledErrors.length > 0) {
      // An error outside any test (an unhandled rejection, a worker crash)
      // makes the recorded outcomes untrustworthy.
      this.announce(
        `${unhandledErrors.length} unhandled error(s) outside the tests: ` +
          "the results are unreliable, no evidence was emitted",
      );
      return;
    }
    if (
      !testModules.some((module) => !module.children.allTests().next().done)
    ) {
      this.announce("the run selected no tests, no evidence was emitted");
      return;
    }
    this.emit(testModules);
    this.announce(`wrote ${this.outputPath()}`);
  }

  private emit(testModules: ReadonlyArray<TestModule>): void {
    const builder = new EvidenceBuilder();
    builder.capsulaRunId(process.env.CAPSULA_RUN_ID);
    for (const [key, value] of Object.entries(this.options.provenance ?? {})) {
      builder.provenance(key, value);
    }
    const caseId = this.options.case ?? DEFAULT_CASE;
    const sources = new SourceCache();
    for (const module of testModules) {
      const project = module.project.name;
      const prefix = project ? `[${project}] ` : "";
      const file = relativeToRoot(this.root(), module.moduleId);
      for (const test of module.children.allTests()) {
        const testId = `${prefix}${file} > ${test.fullName}`;
        const location = test.location;
        if (!location) {
          throw new Error(
            `inka-evidence: ${testId} carries no location; set ` +
              "`test.includeTaskLocation: true` in the vitest config",
          );
        }
        const body = sources.slice(module.moduleId, location, testId, test.name);
        builder.addTest(testId, body, [
          { case: caseId, outcome: mapOutcome(test.result().state, testId) },
        ]);
      }
    }
    // Atomic: a crash mid-write cannot leave a half-written file.
    const output = this.outputPath();
    const scratch = `${output}.tmp`;
    writeFileSync(scratch, builder.finish(), "utf8");
    renameSync(scratch, output);
  }

  private root(): string {
    if (this.vitest === undefined) {
      throw new Error("inka-evidence: the reporter was never initialized");
    }
    return this.vitest.config.root;
  }

  private outputPath(): string {
    const output = this.options.output;
    return isAbsolute(output) ? output : join(this.root(), output);
  }

  private announce(message: string): void {
    this.vitest?.logger.log(`inka-evidence: ${message}`);
  }
}

/**
 * Maps Vitest's final state onto the wire vocabulary; `pending` means the
 * run never reported the test (bail, interrupt) — a missing result.
 */
function mapOutcome(
  state: string,
  testId: string,
): "pass" | "fail" | "skip" | undefined {
  switch (state) {
    case "passed":
      return "pass";
    case "failed":
      return "fail";
    case "skipped":
      return "skip";
    case "pending":
      return undefined;
    default:
      throw new Error(
        `inka-evidence: ${testId} reports the unknown state \`${state}\``,
      );
  }
}

function relativeToRoot(root: string, moduleId: string): string {
  const prefix = root.endsWith("/") ? root : `${root}/`;
  if (!moduleId.startsWith(prefix)) {
    // An absolute fallback would make the test-id machine-specific.
    throw new Error(
      `inka-evidence: ${moduleId} lies outside the vitest root ${root}; ` +
        "its test-id would not be stable",
    );
  }
  return moduleId.slice(prefix.length);
}

/** One parsed source file, cached across the tests it hosts. */
interface ParsedSource {
  text: string;
  lineStarts: number[];
  ast: AstNode;
}

export class SourceCache {
  private readonly cache = new Map<string, ParsedSource>();

  /**
   * The verbatim text of the test call expression at `location`.
   *
   * The resolved call is verified against the task's name: Vitest has been
   * observed to report a wrong location for the tagged-template `each`
   * form, which can land on a neighboring test — a body mismatch must be
   * an error, never silently recorded evidence.
   */
  slice(
    moduleId: string,
    location: { line: number; column: number },
    testId: string,
    testName: string,
  ): string {
    const source = this.load(moduleId);
    const lineStart = source.lineStarts[location.line - 1];
    if (lineStart === undefined) {
      throw new Error(
        `inka-evidence: ${testId} points at ${moduleId}:${location.line}, ` +
          "which is past the end of the file",
      );
    }
    const offset = lineStart + location.column - 1;
    const call = testCallAt(source.ast, offset);
    if (call === null || call.start === null || call.end === null) {
      throw new Error(
        `inka-evidence: no call expression at ${moduleId}:${location.line}:` +
          `${location.column} for ${testId}`,
      );
    }
    verifyCall(call, testId, testName);
    return source.text.slice(call.start, call.end);
  }

  private load(moduleId: string): ParsedSource {
    const cached = this.cache.get(moduleId);
    if (cached !== undefined) {
      return cached;
    }
    const text = readFileSync(moduleId, "utf8");
    const ast = parse(text, {
      sourceType: "unambiguous",
      plugins: pluginsFor(moduleId),
      attachComment: false,
    }) as unknown as AstNode;
    const lineStarts = [0];
    for (let index = 0; index < text.length; index += 1) {
      if (text[index] === "\n") {
        lineStarts.push(index + 1);
      }
    }
    const parsed = { text, lineStarts, ast };
    this.cache.set(moduleId, parsed);
    return parsed;
  }
}

function pluginsFor(path: string): ParserPlugin[] {
  if (path.endsWith(".tsx")) {
    return ["typescript", "jsx"];
  }
  if (/\.(ts|mts|cts)$/.test(path)) {
    return ["typescript"];
  }
  if (path.endsWith(".jsx")) {
    return ["jsx"];
  }
  return [];
}

/**
 * The test call expression owning `offset`.
 *
 * Vitest's task location points inside the call — at the callee for
 * `test(...)`, at the modifier for `test.skip(...)`, at the argument
 * parenthesis for `test.each(...)(...)`. Walking down to the deepest node
 * containing the offset and climbing back out of the callee chain lands
 * on the outermost call of that chain without ever escaping into the
 * enclosing `describe` call, whose span also contains the offset but only
 * through an argument.
 */
function testCallAt(root: AstNode, offset: number): AstNode | null {
  const path: AstNode[] = [];
  let node: AstNode | null = contains(root, offset) ? root : null;
  while (node !== null) {
    path.push(node);
    node = childNodes(node).find((child) => contains(child, offset)) ?? null;
  }
  let index = path.length - 1;
  while (index > 0) {
    const parent = path[index - 1];
    const current = path[index];
    if (
      parent.type === "MemberExpression" ||
      (parent.type === "CallExpression" && parent.callee === current) ||
      (parent.type === "TaggedTemplateExpression" && parent.tag === current)
    ) {
      index -= 1;
      continue;
    }
    break;
  }
  const found = path[index];
  return found !== undefined && found.type === "CallExpression" ? found : null;
}

function contains(node: AstNode, offset: number): boolean {
  return (
    node.start !== null && node.end !== null && offset >= node.start && offset < node.end
  );
}

/**
 * Verifies that the resolved call belongs to the task. A literal first
 * argument must equal the task's name — or, for an `each`/`for` chain,
 * match it placeholder-wise. A dynamic (non-literal) name cannot be
 * verified and is accepted as collected. The tagged-template `each` form
 * is rejected outright: Vitest reports untrustworthy locations for it.
 */
function verifyCall(call: AstNode, testId: string, testName: string): void {
  const chain = calleeChain(call);
  if (chain.some((node) => node.type === "TaggedTemplateExpression")) {
    throw new Error(
      `inka-evidence: ${testId} uses a tagged-template each table, whose ` +
        "reported location is untrustworthy; use the array form of each",
    );
  }
  const argumentList = call.arguments;
  const first = Array.isArray(argumentList) ? argumentList[0] : undefined;
  if (!isNode(first)) {
    throw new Error(
      `inka-evidence: the call resolved for ${testId} takes no arguments, ` +
        "so it cannot be a test declaration; the reported location is " +
        "untrustworthy",
    );
  }
  const literal = literalValue(first);
  if (literal === null) {
    // A template with substitutions is a legitimate dynamic name; every
    // other literal kind (a number, boolean, regexp, ...) cannot be one.
    if (first.type.endsWith("Literal") && first.type !== "TemplateLiteral") {
      throw new Error(
        `inka-evidence: the call resolved for ${testId} does not look like ` +
          "a test declaration; the reported location is untrustworthy",
      );
    }
    // A dynamic name (a variable, a helper call) cannot be verified and
    // is accepted as collected.
    return;
  }
  const eachForm = chain.some(
    (node) =>
      node.type === "MemberExpression" &&
      isNode(node.property) &&
      node.property.type === "Identifier" &&
      (node.property.name === "each" || node.property.name === "for"),
  );
  const matches = eachForm
    ? patternMatches(literal, testName)
    : literal === testName;
  if (!matches) {
    throw new Error(
      `inka-evidence: the call resolved for ${testId} declares ` +
        `${JSON.stringify(literal)}, which does not match the task name ` +
        `${JSON.stringify(testName)}; the reported location is untrustworthy`,
    );
  }
}

/** The nodes along a call's callee chain: `test.skip.each(t)("n", f)`. */
function calleeChain(call: AstNode): AstNode[] {
  const chain: AstNode[] = [];
  let node: AstNode | undefined = call;
  while (node !== undefined) {
    chain.push(node);
    let next: unknown;
    if (node.type === "CallExpression") {
      next = node.callee;
    } else if (node.type === "MemberExpression") {
      next = node.object;
    } else if (node.type === "TaggedTemplateExpression") {
      next = node.tag;
    } else {
      break;
    }
    node = isNode(next) ? next : undefined;
  }
  return chain;
}

/** The compile-time value of a string literal argument, if it has one. */
function literalValue(node: AstNode): string | null {
  if (node.type === "StringLiteral" && typeof node.value === "string") {
    return node.value;
  }
  if (
    node.type === "TemplateLiteral" &&
    Array.isArray(node.expressions) &&
    node.expressions.length === 0 &&
    Array.isArray(node.quasis) &&
    node.quasis.length === 1
  ) {
    const quasi = node.quasis[0];
    if (isNode(quasi)) {
      const value = (quasi.value as { cooked?: unknown } | undefined)?.cooked;
      if (typeof value === "string") {
        return value;
      }
    }
  }
  return null;
}

/**
 * Whether an `each` name pattern can produce `name`: printf tokens and
 * `$property` accesses match any text, everything else matches verbatim.
 */
export function patternMatches(pattern: string, name: string): boolean {
  const printfTokens = "idfsjoc#";
  let regex = "^";
  let index = 0;
  while (index < pattern.length) {
    const char = pattern[index];
    const next = pattern[index + 1];
    if (char === "%" && next === "%") {
      regex += "%";
      index += 2;
      continue;
    }
    if (char === "%" && next !== undefined && printfTokens.includes(next)) {
      regex += "[\\s\\S]*";
      index += 2;
      continue;
    }
    if (char === "$") {
      const access = /^(#|[A-Za-z_$][\w$]*(\.[A-Za-z_$][\w$]*)*)/.exec(
        pattern.slice(index + 1),
      );
      if (access !== null) {
        regex += "[\\s\\S]*";
        index += 1 + access[0].length;
        continue;
      }
    }
    regex += char.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    index += 1;
  }
  regex += "$";
  return new RegExp(regex).test(name);
}

function childNodes(node: AstNode): AstNode[] {
  const children: AstNode[] = [];
  for (const [key, value] of Object.entries(node)) {
    if (key === "loc") {
      continue;
    }
    if (Array.isArray(value)) {
      for (const entry of value) {
        if (isNode(entry)) {
          children.push(entry);
        }
      }
    } else if (isNode(value)) {
      children.push(value);
    }
  }
  return children;
}

function isNode(value: unknown): value is AstNode {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { type?: unknown }).type === "string"
  );
}
