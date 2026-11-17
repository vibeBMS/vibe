#!/usr/bin/env sh
# Build the web version into web/pkg. Requires:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <same as wasm-bindgen in Cargo.lock>
#   clang with the WebAssembly target and llvm-ar (Lua is compiled from C;
#   Apple's clang has no WebAssembly target: use Homebrew's llvm)
# Then serve the web/ folder over HTTP, e.g. `python3 -m http.server -d web 8080`.
set -e
PROFILE=${PROFILE:-web}
cargo build -p vibebms-web --target wasm32-unknown-unknown --profile "$PROFILE"
DIR=$PROFILE; [ "$PROFILE" = dev ] && DIR=debug
TARGET=${CARGO_TARGET_DIR:-target}
wasm-bindgen --target web --no-typescript --out-dir web/pkg "$TARGET/wasm32-unknown-unknown/$DIR/vibebms_web.wasm"
# Lua's errors are Wasm exceptions
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 --enable-exception-handling -o web/pkg/vibebms_web_bg.wasm web/pkg/vibebms_web_bg.wasm
fi
if command -v node >/dev/null 2>&1; then node tools/check_wasm_imports.mjs web/pkg/vibebms_web_bg.wasm; fi
echo "built web/pkg"
