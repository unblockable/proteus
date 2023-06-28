use which::which;

fn main() {
    // Some of our integration tests require external binaries. If we can't find
    // them in our path, then those tests will be excluded from the build and
    // will not be run during `cargo test`.
    let integration_test_bin_deps = vec!["python3", "shadow", "tgen", "tor"];

    let missing = integration_test_bin_deps
        .iter()
        .filter_map(|bin| {
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
