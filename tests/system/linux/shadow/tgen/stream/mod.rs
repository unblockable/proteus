use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use test_each_file::test_each_path;

// Run a tgen test in shadow for each psf in the fixtures directory.
test_each_path! {
    // The test can only be run if our dependencies are satisfied.
    #[ignore, cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
    for ["psf"] in "tests/fixtures" => run_test
}

fn run_test([psf_filepath]: [&Path; 1]) {
    let in_dir = PathBuf::from("tests/system/linux/shadow/tgen/stream");
    let out_dir = super::initialize_test_directory(&in_dir, psf_filepath);
    super::run_shadow_and_assert_result(&out_dir);
}
