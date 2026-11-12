#!/usr/bin/env sh
# Build the web version into web/pkg. Requires:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <same as wasm-bindgen in Cargo.lock>
# Then serve the web/ folder over HTTP, e.g. `python3 -m http.server -d web 8080`.
set -e
PROFILE=${PROFILE:-release}
cargo build -p vibebms-web --target wasm32-unknown-unknown --profile "$PROFILE"
DIR=$PROFILE; [ "$PROFILE" = dev ] && DIR=debug
wasm-bindgen --target web --no-typescript --out-dir web/pkg "target/wasm32-unknown-unknown/$DIR/vibebms_web.wasm"
if command -v wasm-opt >/dev/null 2>&1; then wasm-opt -O3 -o web/pkg/vibebms_web_bg.wasm web/pkg/vibebms_web_bg.wasm; fi
echo "built web/pkg"
