# Commands
## serve-web
Build and serve a development-suitable wasm binary and generate webassembly bindings. The resultant files are put in public/
```bash
rm -rf public
mkdir public
cp index.html public
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir public/pkg target/wasm32-unknown-unknown/release/firesim.wasm
simple-http-server --coep --coop --cors -p 3000 public
```
