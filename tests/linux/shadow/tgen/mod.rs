use std::fs;
use std::path::PathBuf;

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn simple() {
    run_test(&"simple", &"examples/psf/simple.psf");
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn handshake_no_payload() {
    run_test(
        &"handshake_no_payload",
        &"examples/psf/handshake_no_payload.psf",
    );
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn handshake_with_payload() {
    run_test(
        &"handshake_with_payload",
        &"examples/psf/handshake_with_payload.psf",
    );
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn shadowsocks() {
    run_test(&"shadowsocks", &"examples/psf/shadowsocks.psf");
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn shadowsocks_padded() {
    run_test(
        &"shadowsocks_padded",
        &"examples/psf/shadowsocks_padded.psf",
    );
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn tls_mimic() {
    run_test(&"tls_mimic", &"examples/psf/tls_mimic.psf");
}

fn run_test(test_name: &str, psf_filepath: &str) {
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

fn initialize_test_directory(test_name: &str, psf_filepath: &str) -> PathBuf {
    // Set up our paths
    let in_dir_path = PathBuf::from("tests/linux/shadow/tgen");
    let out_dir_path = PathBuf::from(format!("target/{}/{test_name}", in_dir_path.display()));

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
