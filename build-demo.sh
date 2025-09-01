#!/usr/bin/env sh

rm -rf wasm-demo/webapp/*
cd wasm-demo/crate
wasm-pack build --target web
rm -rf ./target
mv pkg ../webapp
cd ../..
CARGO_TARGET_DIR=./target cargo doc --no-deps
cp ./wasm-demo/index.html ./wasm-demo/webapp
mv ./target/doc/*.html ./wasm-demo/webapp
mv ./target/doc/*.js ./wasm-demo/webapp
mv ./target/doc/search.desc ./wasm-demo/webapp
mv ./target/doc/static.files ./wasm-demo/webapp
mv ./target/doc/trait.impl ./wasm-demo/webapp
mv ./target/doc/macrosia ./wasm-demo/webapp/doc
mv ./target/doc/*.js ./wasm-demo/webapp
mv ./target/doc/src ./wasm-demo/webapp/doc
