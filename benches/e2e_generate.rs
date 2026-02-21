use divan;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let config = BenchConfig::from_env();
    run_generate(&config, "warm-up");
    divan::main();
}

#[derive(Debug)]
struct BenchConfig {
    workspace: PathBuf,
    thought_bin: PathBuf,
}

impl BenchConfig {
    fn from_env() -> Self {
        let workspace = std::env::var_os("THOUGHT_BENCH_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("THOUGHT_BENCH_WORKSPACE must be set for e2e benchmarks"));
        let thought_bin = std::env::var_os("THOUGHT_BENCH_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("THOUGHT_BENCH_BIN must be set for e2e benchmarks"));

        assert!(
            workspace.join("Thought.toml").exists(),
            "Benchmark workspace is missing Thought.toml at {}.",
            workspace.display()
        );
        assert!(
            thought_bin.exists(),
            "Thought benchmark binary not found at {}.",
            thought_bin.display()
        );

        Self {
            workspace,
            thought_bin,
        }
    }
}

fn run_generate(config: &BenchConfig, stage: &str) {
    let build_dir = config.workspace.join("build");
    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir)
            .unwrap_or_else(|err| panic!("Failed to clean {}: {err}", build_dir.display()));
    }

    let status = Command::new(&config.thought_bin)
        .arg("generate")
        .current_dir(&config.workspace)
        .status()
        .unwrap_or_else(|err| {
            panic!(
                "Failed to launch {} from {} during {stage}: {err}",
                config.thought_bin.display(),
                config.workspace.display()
            )
        });

    assert!(
        status.success(),
        "thought generate failed with status {status} in {} during {stage}",
        config.workspace.display()
    );

    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir)
            .unwrap_or_else(|err| panic!("Failed to clean {}: {err}", build_dir.display()));
    }
}

#[divan::bench]
fn thought_generate_e2e() {
    let config = BenchConfig::from_env();
    run_generate(&config, "measurement");
}
