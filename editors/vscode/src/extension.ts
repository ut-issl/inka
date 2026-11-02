// The client half is deliberately thin: launch the server named by
// `inka.serverPath`, hand it every inka document, and forward watched-file
// events for the non-inka inputs a project reads (manifest, values, evidence,
// approvals) so diagnostics follow disk changes too.

import { ExtensionContext, window, workspace } from "vscode";
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

  startClient(clientOptions);
  context.subscriptions.push(
    workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("inka.serverPath")) {
        void restartClient(clientOptions);
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

function serverCommand(): string {
  const configured =
    workspace.getConfiguration("inka").get<string>("serverPath") ?? "";
  return configured.trim() === "" ? "inka-lsp" : configured.trim();
}

function startClient(clientOptions: LanguageClientOptions): void {
  const command = serverCommand();
  client = new LanguageClient(
    "inka",
    "inka language server",
    { command, args: [] },
    clientOptions,
  );
  client.start().catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    void window.showErrorMessage(
      `inka-lsp failed to start (inka.serverPath = "${command}"): ${message}`,
    );
  });
}

async function restartClient(
  clientOptions: LanguageClientOptions,
): Promise<void> {
  if (client) {
    try {
      await client.stop();
    } catch {
      // It never ran; there is nothing to stop.
    }
  }
  startClient(clientOptions);
}
