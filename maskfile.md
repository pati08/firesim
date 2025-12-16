# Commands
## serve
Build and serve a development-suitable wasm binary and generate webassembly bindings. The resultant files are put in public/
```bash
mask build
simple-http-server --coep --coop --cors -i -p 3000 public
```

## build
Build the webassembly binary and generate JS bindings
```bash
rm -rf public
cp -r front public
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir public/pkg target/wasm32-unknown-unknown/release/firesim.wasm
```

## dev
Watch files and rebuild and serve when they change
```bash
watchexec -e js,css,html,rs,toml -o restart mask serve
```
