use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use test_each_file::test_each_path;

// Run a tgen test in shadow for each psf in the fixtures directory.
test_each_path! {
    // The test can only be run if our dependencies are satisfied.
    #[ignore, cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor, have_python3))]
    for ["psf"] in "tests/fixtures" => run_test
}

fn run_test([psf_filepath]: [&Path; 1]) {
    let test_name = psf_filepath.file_stem().unwrap();
    let (rel_test_dir, rel_run_dir) = initialize_test_directory(test_name, psf_filepath);

    let rel_run_dir_str = rel_run_dir.to_string_lossy();
    let abs_test_dir = fs::canonicalize(rel_test_dir).expect("Canonicalize path");
    let abs_test_dir_str = abs_test_dir.to_string_lossy();

    let template_opt = format!("--template-directory={abs_test_dir_str}/shadow.data.template",);
    // We disable CPU pinning because we are running many Shadow sims at the same time and we
    // don't want them all to pin to the same set of CPUs.
    let args = [
        template_opt.as_str(),
        "--parallelism=4",
        "--use-cpu-pinning=false",
        "shadow.yaml",
    ];
    assert!(super::run_shadow(&rel_run_dir, args).success());

    let client = PathBuf::from(format!(
        "{rel_run_dir_str}/shadow.data/hosts/client/tgen.1006.stdout",
    ));
    let server = PathBuf::from(format!(
        "{rel_run_dir_str}/shadow.data/hosts/server/tgen.1000.stdout",
    ));

    assert_eq!(super::count_tgen_stream_successes(client), 5);
    assert_eq!(super::count_tgen_stream_successes(server), 5);
}

fn initialize_test_directory(test_name: &OsStr, psf_filepath: &Path) -> (PathBuf, PathBuf) {
    // Set up our paths
    let in_dir_path = PathBuf::from("tests/system/linux/shadow/tor");
    let out_dir_path = PathBuf::from("target").join(&in_dir_path).join(test_name);

    // We need to write the proteus bin and PSF paths into the config files.
    let bin_path = PathBuf::from(test_bin::get_test_bin("proteus").get_program());
    let psf_path = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // Shadow needs a clear working directory.
    super::remove_and_create_all(out_dir_path.clone());

    // Copy the tgen client config. We keep the template suffix because we only
    // partially instantiate it here and the rest happens during the sim.
    {
        let conf = "torrc.template";
        let in_path = PathBuf::from(format!("{}/{conf}", in_dir_path.display()));
        let out_path = PathBuf::from(format!("{}/{conf}", out_dir_path.display()));
        let replacements = vec![("${PSFPATH}", psf_path.to_str().unwrap())];
        super::copy_test_file_with_replace(&in_path, &out_path, replacements);
    }

    // Copy the shadow config, instantiating the template variables.
    {
        let in_path = PathBuf::from(format!("{}/shadow.yaml.template", in_dir_path.display()));
        let out_path = PathBuf::from(format!("{}/shadow.yaml", out_dir_path.display()));
        let replacements = vec![
            ("${PSFPATH}", psf_path.to_str().unwrap()),
            ("${PROTEUSBINPATH}", bin_path.to_str().unwrap()),
        ];
        super::copy_test_file_with_replace(&in_path, &out_path, replacements);
    }

    (in_dir_path, out_dir_path)
}
