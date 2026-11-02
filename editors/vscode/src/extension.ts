// The client half is deliberately thin: launch the server named by
// `inka.serverPath`, hand it every inka document, and forward watched-file
// events for the non-inka inputs a project reads (manifest, values, evidence,
// approvals) so diagnostics follow disk changes too.

import { ExtensionContext, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext): void {
  const command =
    workspace.getConfiguration("inka").get<string>("serverPath") ?? "inka-lsp";
  const serverOptions: ServerOptions = { command, args: [] };
  const watcher = workspace.createFileSystemWatcher(
    "**/*.{inka,toml,json,jsonl}",
  );
  context.subscriptions.push(watcher);
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ language: "inka" }],
    synchronize: { fileEvents: watcher },
  };
  client = new LanguageClient(
    "inka",
    "inka language server",
    serverOptions,
    clientOptions,
  );
  void client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
