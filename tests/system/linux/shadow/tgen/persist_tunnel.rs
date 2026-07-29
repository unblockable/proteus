use std::path::{Path, PathBuf};

use test_each_file::test_each_path;

use crate::system::linux::shadow::run_shadow_and_assert_result;

// Run a tgen test in shadow for each psf in the fixtures directory.
test_each_path! {
    // The test can only be run if our dependencies are satisfied.
    #[ignore, cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
    for ["psf"] in "tests/fixtures" => run_test
}

fn run_test([psf_filepath]: [&Path; 1]) {
    let rel_test_src = PathBuf::from("tests/system/linux/shadow/tgen/persist_tunnel");
    let rel_test_dst =
        super::initialize_test_directory(&rel_test_src, psf_filepath, "true", "tunnel");
    run_shadow_and_assert_result(&rel_test_dst, None, 1000, 1002);
}
