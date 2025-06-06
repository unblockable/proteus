use which::which;

fn main() {
    // Some of our integration tests require external binaries. If we can't find
    // them in our path, then those tests will be excluded from the build and
    // will not be run during `cargo test`.
    let integration_test_bin_deps = ["python3", "shadow", "tgen", "tor"];

    let missing = integration_test_bin_deps
        .iter()
        .filter_map(|bin| {
            // When linting, assume all tools are available.
            println!("cargo:rustc-check-cfg=cfg(have_{bin})");
            // When not lining, build tests for only the tools we have.
            if which(bin).is_ok() {
                println!("cargo:rustc-cfg=have_{bin}");
                None
            } else {
                Some(bin.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    if !missing.is_empty() {
        println!(
            "cargo:warning=Cannot find [{missing}] in path, \
                  some tests will be skipped."
        );
    }
}
