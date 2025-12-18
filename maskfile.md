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
wasm-pack build --target web --profiling --no-opt --out-dir public/pkg
```

## dev
Watch files and rebuild and serve when they change
```bash
watchexec -e js,css,html,rs,toml,wgsl -o restart mask serve
```
