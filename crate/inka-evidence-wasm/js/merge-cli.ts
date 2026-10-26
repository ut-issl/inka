#!/usr/bin/env node
/**
 * inka-evidence-merge: merge per-configuration evidence files into one.
 *
 * Inputs are `[LABEL=]FILE` pairs; a label prefixes every case id of its
 * file as `label/case`. The merge semantics live in the shared Rust crate
 * (through the wasm binding) — this script only parses arguments and does
 * the file I/O.
 */

import { readFileSync, writeFileSync } from "node:fs";

import { type MergeInput, merge } from "../pkg/inka_evidence_wasm.js";

/**
 * Splits one `[LABEL=]FILE` argument. The part before the first `=` is a
 * label only when it contains no path separator, so a file name with an
 * `=` still works when written with a leading `./`.
 */
function splitInput(raw: string): { label?: string; file: string } {
  const separator = raw.indexOf("=");
  if (separator > 0) {
    const label = raw.slice(0, separator);
    if (!label.includes("/") && !label.includes("\\")) {
      return { label, file: raw.slice(separator + 1) };
    }
  }
  return { file: raw };
}

function fail(message: string): never {
  console.error(`error: ${message}`);
  process.exit(1);
}

function main(argv: string[]): void {
  let output = "-";
  const provenance: Record<string, string> = {};
  const inputs: MergeInput[] = [];

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--output") {
      index += 1;
      const value = argv[index];
      if (value === undefined) {
        fail("--output needs a path");
      }
      output = value;
    } else if (argument === "--provenance") {
      index += 1;
      const value = argv[index];
      const separator = value?.indexOf("=") ?? -1;
      if (value === undefined || separator <= 0) {
        fail(`provenance entry ${JSON.stringify(value)} is not key=value`);
      }
      provenance[value.slice(0, separator)] = value.slice(separator + 1);
    } else if (argument.startsWith("--")) {
      fail(`unknown option ${argument}`);
    } else {
      const { label, file } = splitInput(argument);
      let text;
      try {
        text = readFileSync(file, "utf8");
      } catch (error) {
        fail(
          `cannot read ${file}: ${error instanceof Error ? error.message : error}`,
        );
      }
      inputs.push(label === undefined ? { text } : { label, text });
    }
  }
  if (inputs.length === 0) {
    fail(
      "usage: inka-evidence-merge [--output PATH] [--provenance K=V]... [LABEL=]FILE...",
    );
  }

  let merged;
  try {
    merged = merge(inputs, {
      provenance,
      capsulaRunId: process.env.CAPSULA_RUN_ID,
    });
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }

  if (output === "-") {
    process.stdout.write(merged);
  } else {
    try {
      writeFileSync(output, merged, "utf8");
    } catch (error) {
      fail(
        `cannot write ${output}: ${error instanceof Error ? error.message : error}`,
      );
    }
  }
}

main(process.argv.slice(2));
