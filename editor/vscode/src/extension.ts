import * as path from "node:path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
  const configured = vscode.workspace.getConfiguration("atool").get<string>("server.path");
  const serverModule = configured && configured.length > 0
    ? configured
    : context.asAbsolutePath(path.join("..", "atool-lsp", "dist", "server.js"));

  const serverOptions: ServerOptions = {
    run:   { module: serverModule, transport: TransportKind.ipc },
    debug: { module: serverModule, transport: TransportKind.ipc, options: { execArgv: ["--nolazy", "--inspect=6011"] } },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "json", pattern: "**/atool.json" },
      { scheme: "file", language: "jsonc", pattern: "**/atool.json" },
    ],
  };

  client = new LanguageClient("atool", "Alexandria atool", serverOptions, clientOptions);
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
