// The client half is deliberately thin: launch the server, hand it every
// inka document, and forward watched-file events for the non-inka inputs a
// project reads (manifest, values, evidence, approvals) so diagnostics
// follow disk changes too.
//
// The server is found in this order: the `inka.serverPath` setting when it
// is set; in an Extension Development Host only, the `INKA_LSP_DEV_SERVER`
// environment variable (the launch configuration points it at the
// repository's `target/debug/inka-lsp`); the `inka-lsp` bundled under the
// extension's own `server/` directory (the platform-specific .vsix of a
// release carries one); and finally `inka-lsp` on PATH.

import { existsSync } from "node:fs";
import * as path from "node:path";

import { ExtensionContext, ExtensionMode, window, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  State,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext): void {
  // The inputs a project reads besides .inka sources: the manifest and
  // values (*.toml), approvals (*.jsonl), and delivered evidence. Evidence
  // JSON is only watched under an evidence/ directory — a bare **/*.json
  // would forward every build artifact under target/ to the server.
  const watchers = [
    workspace.createFileSystemWatcher("**/*.inka"),
    workspace.createFileSystemWatcher("**/*.toml"),
    workspace.createFileSystemWatcher("**/*.jsonl"),
    workspace.createFileSystemWatcher("**/evidence/**/*.json"),
  ];
  for (const watcher of watchers) {
    context.subscriptions.push(watcher);
  }
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ language: "inka" }],
    synchronize: { fileEvents: watchers },
  };

  startClient(context, clientOptions);
  context.subscriptions.push(
    workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("inka.serverPath")) {
        void restartClient(context, clientOptions);
      }
    }),
  );
}

export function deactivate(): Thenable<void> | undefined {
  // Stopping a client that never reached Running throws instead of no-oping.
  if (client?.state === State.Running) {
    return client.stop();
  }
  return undefined;
}

function serverCommand(context: ExtensionContext): string {
  const configured = (
    workspace.getConfiguration("inka").get<string>("serverPath") ?? ""
  ).trim();
  if (configured !== "") {
    return configured;
  }
  // Read only while debugging the extension, so an installed extension
  // never picks up a server from whatever environment VS Code started in.
  const development = (process.env.INKA_LSP_DEV_SERVER ?? "").trim();
  if (
    context.extensionMode === ExtensionMode.Development &&
    development !== ""
  ) {
    return development;
  }
  const bundled = path.join(
    context.extensionPath,
    "server",
    process.platform === "win32" ? "inka-lsp.exe" : "inka-lsp",
  );
  return existsSync(bundled) ? bundled : "inka-lsp";
}

function startClient(
  context: ExtensionContext,
  clientOptions: LanguageClientOptions,
): void {
  const command = serverCommand(context);
  client = new LanguageClient(
    "inka",
    "inka language server",
    { command, args: [] },
    clientOptions,
  );
  client.start().catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    void window.showErrorMessage(
      `inka-lsp failed to start (${command}): ${message}`,
    );
  });
}

async function restartClient(
  context: ExtensionContext,
  clientOptions: LanguageClientOptions,
): Promise<void> {
  if (client) {
    try {
      await client.stop();
    } catch {
      // It never ran; there is nothing to stop.
    }
  }
  startClient(context, clientOptions);
}
