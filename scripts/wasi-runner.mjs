// Runs a wasm32-wasip1 binary (e.g. `cargo test` output) with Node's WASI.
// Used as the cargo runner on machines that cannot execute freshly built
// native binaries (see docs/development.md).
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";
import process from "node:process";

const [, , wasmPath, ...args] = process.argv;
const wasi = new WASI({
  version: "preview1",
  args: [wasmPath, ...args],
  env: process.env,
  returnOnExit: true,
});
const module = await WebAssembly.compile(await readFile(wasmPath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
process.exit(wasi.start(instance));
