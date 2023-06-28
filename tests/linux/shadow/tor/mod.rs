use std::fs;
use std::path::PathBuf;

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor, have_python3))]
fn simple() {
    run_test(&"simple", &"examples/psf/simple.psf");
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor, have_python3))]
fn shadowsocks() {
    run_test(&"shadowsocks", &"examples/psf/shadowsocks.psf");
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor, have_python3))]
fn shadowsocks_padded() {
    run_test(
        &"shadowsocks_padded",
        &"examples/psf/shadowsocks_padded.psf",
    );
}

fn run_test(test_name: &str, psf_filepath: &str) {
    let (rel_test_dir, rel_run_dir) = initialize_test_directory(test_name, psf_filepath);
    let rel_run_dir_str = rel_run_dir.to_string_lossy();
    let abs_test_dir = fs::canonicalize(rel_test_dir).expect("Canonicalize path");
    let abs_test_dir_str = abs_test_dir.to_string_lossy();

    let template_opt = format!("--template-directory={abs_test_dir_str}/shadow.data.template",);
    let args = [template_opt.as_str(), "--parallelism=4", "shadow.yaml"];
    assert!(super::run_shadow(&rel_run_dir, args).success());

    let client = PathBuf::from(format!(
        "{rel_run_dir_str}/shadow.data/hosts/client/tgen.1004.stdout",
    ));
    let server = PathBuf::from(format!(
        "{rel_run_dir_str}/shadow.data/hosts/server/tgen.1000.stdout",
    ));

    assert_eq!(super::count_tgen_stream_successes(client), 5);
    assert_eq!(super::count_tgen_stream_successes(server), 5);
}

fn initialize_test_directory(test_name: &str, psf_filepath: &str) -> (PathBuf, PathBuf) {
    // Set up our paths
    let in_dir_path = PathBuf::from("tests/linux/shadow/tor");
    let out_dir_path = PathBuf::from(format!("target/{}/{test_name}", in_dir_path.display()));

    // We need to write the proteus bin and PSF paths into the config files.
    let bin_path = fs::canonicalize("target/debug/proteus").expect("Canonicalize path");
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
