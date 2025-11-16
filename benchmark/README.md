# Thought vs Hexo Benchmark

This directory hosts a reproducible benchmark that compares the site
generation throughput of **Thought** and **Hexo** on the same data set
and template.

## What it does

1. Generates a synthetic workspace with a configurable number of Markdown
   articles (default: 10,000) plus matching metadata.
2. Creates an equivalent Hexo blog where each post uses the exact same
   Markdown and a hand-written theme whose HTML mirrors
   `themes/zenflow` (index + article templates are byte-for-byte twins).
3. Prebuilds the Zenflow plugin to `main.wasm` **before** any measurements
   so the expensive WASM compilation phase does not distort Thought's score.
4. Runs both generators and records their wall-clock time.

All temporary files live under `benchmark/workdir` and are ignored by git so
the huge fixture never pollutes the repository.

## Requirements

- Rust toolchain + [`rust-script`](https://rust-script.org/) (`cargo install rust-script`)
- `wasm32-wasip2` target (`rustup target add wasm32-wasip2`)
- Node.js with the Hexo CLI available on `PATH` (`npm install -g hexo-cli`)
- A compiled Thought binary (default path: `target/release/thought`)

The script can reuse any existing `main.wasm` artifact; otherwise it will run
`cargo build --release --target wasm32-wasip2` inside `themes/zenflow` once.
Hexo's npm dependencies are installed automatically into `benchmark/cache`
during the first run so subsequent benchmarks reuse the cached `node_modules`.

## Running the benchmark

From the repository root:

```bash
rust-script benchmark/run.rs
```

Useful flags:

| Flag | Default | Description |
| --- | --- | --- |
| `--articles <N>` | `10000` | Number of posts to synthesize |
| `--thought-bin <path>` | `target/release/thought` | Explicit Thought binary |
| `--hexo-bin <path>` | `hexo` | Path to the Hexo CLI |
| `--keep-data` | off | Keep the generated workdir for manual inspection |
| `--json` | off | Emit a machine-readable JSON summary in addition to stdout |

A sample run prints:

```
Generated 10,000 articles (~450 MB of markdown)
Thought: 4.32s (2314 posts/s)
Hexo:   17.88s (559 posts/s)
```

(Numbers will vary with hardware.)

## Output

The script writes `benchmark/results/latest.json` whenever `--json` is set.
You can archive prior runs by copying files from this folder.

## Fairness notes

- Both generators consume the same Markdown files.
- The Hexo theme under `benchmark/hexo-theme` emits the identical HTML
  structure as Zenflow so layout costs are comparable.
- Thought's WASM compilation is forced to happen ahead-of-time (outside
  timing) by baking the resulting `main.wasm` next to `Plugin.toml`.
- Search indexing is enabled for both generators: Thought always bundles
  `thought-search.{wasm,js}` while Hexo ships `hexo-generator-searchdb`
  preconfigured in `_config.yml`.
