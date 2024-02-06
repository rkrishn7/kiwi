TODO: Add docs

Cmd:

```
docker run -p 8000:8000 -v $(pwd)/kiwi.yml:/etc/kiwi/config/kiwi.yml \
    -v $(pwd)/target/wasm32-wasi/debug/intercept.wasm:/etc/kiwi/hook/intercept.wasm \
    kiwi:latest
```
