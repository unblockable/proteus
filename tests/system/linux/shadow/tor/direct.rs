use std::fs;
use std::path::{Path, PathBuf};

use test_each_file::test_each_path;

use crate::system::linux::shadow::run_shadow_and_assert_result;

// Run a tor test in shadow for each psf in the fixtures directory.
test_each_path! {
    // The test can only be run if our dependencies are satisfied.
    #[ignore, cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor, have_python3))]
    for ["psf"] in "tests/fixtures" => run_test
}

fn run_test([psf_filepath]: [&Path; 1]) {
    let rel_path = PathBuf::from("tests/system/linux/shadow/tor/direct");

    let (rel_test_dst, rel_test_src_parent) =
        super::initialize_test_directory(&rel_path, psf_filepath, "false");

    let abs_test_dir = fs::canonicalize(rel_test_src_parent).expect("Canonicalize path");
    let template_dir = abs_test_dir.join("shadow.data.template");

    run_shadow_and_assert_result(&rel_test_dst, Some(template_dir), 1000, 1006);
}
