use std::fs;
use std::path::{Path, PathBuf};

mod pt;
mod stream;
mod tunnel;

fn initialize_test_directory(in_dir: &PathBuf, psf_filepath: &Path) -> PathBuf {
    let test_name = psf_filepath.file_stem().unwrap();

    // Set up our test dir.
    let in_dir_parent = in_dir.parent().unwrap().display();
    let out_dir = PathBuf::from("target").join(&in_dir).join(test_name);

    // We need to write the proteus bin and PSF paths into the config files.
    let bin_path = PathBuf::from(test_bin::get_test_bin("proteus").get_program());
    let psf_path = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // Use the common tgen client and server confs.
    let server_path = fs::canonicalize(format!("{in_dir_parent}/tgen-server.graphml"))
        .expect("Canonicalize path");
    let client_path = fs::canonicalize(format!("{in_dir_parent}/tgen-client.graphml"))
        .expect("Canonicalize path");

    // Shadow needs a clear working directory.
    super::remove_and_create_all(out_dir.clone());

    // Copy the shadow config, instantiating the template variables.
    {
        let in_path = PathBuf::from(format!("{}/shadow.yaml.template", in_dir.display()));
        let out_path = PathBuf::from(format!("{}/shadow.yaml", out_dir.display()));
        let replacements = vec![
            ("${TGENSERVERCONF}", server_path.to_str().unwrap()),
            ("${TGENCLIENTCONF}", client_path.to_str().unwrap()),
            ("${PSFPATH}", psf_path.to_str().unwrap()),
            ("${PROTEUSBINPATH}", bin_path.to_str().unwrap()),
        ];
        super::copy_test_file_with_replace(&in_path, &out_path, replacements);
    }

    out_dir
}

fn run_shadow_and_assert_result(run_dir: &Path) {
    let run_dir_str = run_dir.to_string_lossy();

    // We disable CPU pinning because we are running many Shadow sims at the same time and we
    // don't want them all to pin to the same set of CPUs.
    assert!(
        super::run_shadow(
            &run_dir,
            ["--parallelism=4", "--use-cpu-pinning=false", "shadow.yaml"]
        )
        .success()
    );

    let client = PathBuf::from(format!(
        "{run_dir_str}/shadow.data/hosts/client/tgen.1002.stdout",
    ));
    let server = PathBuf::from(format!(
        "{run_dir_str}/shadow.data/hosts/server/tgen.1000.stdout",
    ));

    assert_eq!(super::count_tgen_stream_successes(client), 5);
    assert_eq!(super::count_tgen_stream_successes(server), 5);
}
