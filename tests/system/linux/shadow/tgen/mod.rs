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
    let test_name = psf_filepath.file_stem().unwrap();
    let run_dir = initialize_test_directory(test_name, psf_filepath);
    let run_dir_str = run_dir.to_string_lossy();

    assert!(super::run_shadow(&run_dir, ["--parallelism=4", "shadow.yaml"]).success());

    let client = PathBuf::from(format!(
        "{run_dir_str}/shadow.data/hosts/client/tgen.1003.stdout",
    ));
    let server = PathBuf::from(format!(
        "{run_dir_str}/shadow.data/hosts/server/tgen.1000.stdout",
    ));

    assert_eq!(super::count_tgen_stream_successes(client), 5);
    assert_eq!(super::count_tgen_stream_successes(server), 5);
}

fn initialize_test_directory(test_name: &OsStr, psf_filepath: &Path) -> PathBuf {
    // Set up our paths
    let in_dir_path = PathBuf::from("tests/system/linux/shadow/tgen");
    let out_dir_path = PathBuf::from(format!("target/{}/{test_name:?}", in_dir_path.display()));

    // We need to write the proteus bin and PSF paths into the config files.
    let bin_path = fs::canonicalize("target/debug/proteus").expect("Canonicalize path");
    let psf_path = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // The tgen server conf does not change, so just use the one from test_dir_in.
    let server_path =
        fs::canonicalize(format!("{}/tgen-server.graphml.xml", in_dir_path.display()))
            .expect("Canonicalize path");

    // Shadow needs a clear working directory.
    super::remove_and_create_all(out_dir_path.clone());

    // Copy the tgen client config. We keep the template suffix because we only
    // partially instantiate it here and the rest happens during the sim.
    {
        let conf = "tgen-client.graphml.xml.template";
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
            ("${TGENSERVERCONF}", server_path.to_str().unwrap()),
            ("${PSFPATH}", psf_path.to_str().unwrap()),
            ("${PROTEUSBINPATH}", bin_path.to_str().unwrap()),
        ];
        super::copy_test_file_with_replace(&in_path, &out_path, replacements);
    }

    out_dir_path
}
