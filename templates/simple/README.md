# Simple template

Run Rust producer in one terminal:

```sh
cargo run --example fake_indexer --features viz
```

Run dashboard in another:

```sh
npm install
npm run dev
```

Vite proxies `/ws` and `/health` to `127.0.0.1:9999`.
