#!/usr/bin/env sh

cd wasm-demo
wasm-pack build --target web
cd ..
cargo doc --no-deps
rm -rf ./wasm-demo/target
cp -r ./target/doc/* ./wasm-demo
mv wasm-demo/macrosia wasm-demo/doc
