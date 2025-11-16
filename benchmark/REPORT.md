# Thought vs Hexo Benchmark Report

Date: 2025-02-14  
Host: Apple silicon macOS (Darwin 25.0.0)  
Rust: `rustc 1.85.0-nightly (release)` (via `cargo build --release`)  
Node: `v24.10.0` with `hexo-cli 4.3.2`

## Methodology

- Harness: `rust-script benchmark/run.rs`
- Dataset: 10,000 synthetic Markdown articles, 12 paragraphs each, identical for both engines
- Theme: `themes/zenflow` precompiled to `main.wasm`; Hexo theme in `benchmark/hexo-theme` mirrors Zenflow templates
- Commands timed:
  - `thought generate` (workspace under `benchmark/workdir/thought-workspace`)
  - `hexo generate --silent` (workspace under `benchmark/workdir/hexo-workspace`)
- Cache warm-up: first run builds Zenflow to wasm and installs npm deps into `benchmark/cache`
- Filesystem cleaned before measurement run; benchmark output directories ignored via `.gitignore`

## Results

| Generator | Time (s) | Throughput (posts/s) | Notes |
|-----------|---------:|---------------------:|-------|
| Thought   | 6.19     | 1,615.6              | `target/release/thought` binary, WASM prebuilt |
| Hexo      | 15.12    |   661.5              | `hexo generate --silent`, `node_modules` provisioned from cache |

## Observations

1. Thought processed the corpus ~2.4× faster than Hexo on this workload.
2. The majority of total runtime was spent in generation (article creation and indexing); dataset creation and dependency installs are done outside the timed sections.
3. The npm dependency cache allows repeated runs without reinvoking `npm install`, keeping Hexo’s timing focused on `hexo generate`.

## Reproduction Steps

```bash
cargo build --release
rust-script benchmark/run.rs --articles 10000
```

Optional flags:

- `--keep-data` to inspect the generated workspaces after the run.
- `--json` to capture a machine-readable summary in `benchmark/results/latest.json`.
- `--force-theme-build` if the Zenflow wasm artifact should be rebuilt before benchmarking.

Ensure prerequisites in `benchmark/README.md` are met (rust-script, wasm target, Hexo CLI). For different dataset sizes, change `--articles N` and note the new counts in subsequent reports.
