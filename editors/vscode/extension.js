/*
	Courier for VS Code: starts `courier lsp`, the compiler's own language server, for every .courier
	file, so errors, sizes on hover and inline, definitions, references, renames, completion and the
	outline come from the compiler that builds the game. Saving a schema compiles its folder unless
	courier.compileOnSave is off, which the server reads when it starts. The binary is the courier.path setting, `courier` on PATH by default, as
	Rokit installs it. If it cannot start, the extension says so and highlighting still works

	When courier.path names a file, the server runs from a copy in the extension's storage, so the
	file itself stays free for cargo to rebuild, and the extension watches it: once it changes and
	has stayed the same for a second, the server restarts on a fresh copy. Older copies are removed
	once nothing runs them. "Courier: Restart Server" restarts by hand, and changing courier.path
	restarts too
*/

const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;
let storage;
let watched;
let settle;

function configured() {
	return vscode.workspace.getConfiguration("courier").get("path") || "courier";
}

function isFile(command) {
	try {
		return path.isAbsolute(command) && fs.statSync(command).isFile();
	} catch {
		return false;
	}
}

function shadow(command) {
	if (!isFile(command)) {
		return command;
	}
	fs.mkdirSync(storage, { recursive: true });
	const copy = path.join(storage, `courier-${Date.now()}${path.extname(command)}`);
	fs.copyFileSync(command, copy);
	for (const name of fs.readdirSync(storage)) {
		const old = path.join(storage, name);
		if (old !== copy && name.startsWith("courier-")) {
			try {
				fs.unlinkSync(old);
			} catch {}
		}
	}
	return copy;
}

async function start() {
	const command = configured();
	const run = shadow(command);
	client = new LanguageClient(
		"courier",
		"Courier",
		{ command: run, args: ["lsp"] },
		{
			documentSelector: [{ scheme: "file", language: "courier" }],
			initializationOptions: {
				compileOnSave: vscode.workspace.getConfiguration("courier").get("compileOnSave", true),
			},
		}
	);
	try {
		await client.start();
	} catch (problem) {
		client = undefined;
		vscode.window.showWarningMessage(
			`Courier: could not start "${command} lsp" (${problem.message}). Install the courier compiler or set courier.path.`
		);
	}
}

async function restart() {
	if (client) {
		const running = client;
		client = undefined;
		await running.stop().catch(() => {});
	}
	await start();
	watch();
}

function watch() {
	if (watched) {
		fs.unwatchFile(watched);
		watched = undefined;
	}
	const command = configured();
	if (!isFile(command)) {
		return;
	}
	watched = command;
	fs.watchFile(command, { interval: 500 }, (now, before) => {
		if (now.mtimeMs === before.mtimeMs || now.size === 0) {
			return;
		}
		clearTimeout(settle);
		settle = setTimeout(() => restart(), 1000);
	});
}

async function activate(context) {
	storage = context.globalStorageUri.fsPath;
	context.subscriptions.push(
		vscode.commands.registerCommand("courier.restart", () => restart()),
		vscode.workspace.onDidChangeConfiguration((change) => {
			if (change.affectsConfiguration("courier.path") || change.affectsConfiguration("courier.compileOnSave")) {
				restart();
			}
		})
	);
	await start();
	watch();
}

async function deactivate() {
	if (watched) {
		fs.unwatchFile(watched);
	}
	clearTimeout(settle);
	if (client) {
		await client.stop();
	}
}

module.exports = { activate, deactivate };
