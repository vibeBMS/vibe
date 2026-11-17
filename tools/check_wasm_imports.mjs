// The web build's module may import only from wasm-bindgen's glue. An
// import from "env" (or WASI) is a C function nothing defines: rustc links
// wasm with undefined symbols allowed, so it would only fail when the page
// loads. Usage: node tools/check_wasm_imports.mjs [web/pkg/vibebms_web_bg.wasm]
import { readFileSync } from "node:fs";

const file = process.argv[2] ?? "web/pkg/vibebms_web_bg.wasm";
const imports = WebAssembly.Module.imports(new WebAssembly.Module(readFileSync(file)));
const bad = imports.filter((i) => i.module === "env" || i.module.startsWith("wasi"));
if (bad.length > 0) {
  console.error(`${file}: undefined imports: ${bad.map((i) => `${i.module}.${i.name}`).join(" ")}`);
  process.exit(1);
}
console.log(`${file}: ${imports.length} imports, none undefined`);
