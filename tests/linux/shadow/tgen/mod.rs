use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
#[ignore]
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_python3))]
fn simple() {
    run_test(&"simple", &"examples/psf/simple.psf");
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

fn run_test(test_name: &str, psf_filepath: &str) {
    let run_dir = initialize_test_directory(test_name, psf_filepath);

    let log = File::create(format!("{}/shadow.log", run_dir.display())).expect("Create log file");
    let mut cmd = Command::new("shadow")
        .current_dir(run_dir.clone())
        .arg("shadow.yaml")
        .stdout(Stdio::from(log.try_clone().expect("Clone log")))
        .stderr(Stdio::from(log))
        .spawn()
        .expect("Starting shadow process");
    let status = cmd.wait().expect("Shadow not running");

    assert!(status.success());

    let tgen_log_path = PathBuf::from(format!(
        "{}/shadow.data/hosts/client/tgen.1003.stdout",
        run_dir.display()
    ));
    assert_eq!(count_stream_successes(tgen_log_path), 5);
}

fn initialize_test_directory(test_name: &str, psf_filepath: &str) -> PathBuf {
    let test_dir_in = PathBuf::from("tests/linux/shadow/tgen");
    let test_dir_out = PathBuf::from(format!("target/{}/{test_name}", test_dir_in.display()));

    let bin_path = fs::canonicalize("target/debug/proteus").expect("Canonicalize path");

    // Shadow needs a clear working directory.
    if test_dir_out.exists() {
        fs::remove_dir_all(test_dir_out.clone()).expect("Remove test output dir");
    }
    fs::create_dir_all(test_dir_out.clone()).expect("Create test output dir");

    // The tgen server conf does not change, so just use the one from test_dir_in.
    let tgen_server_conf =
        fs::canonicalize(format!("{}/tgen-server.graphml.xml", test_dir_in.display()))
            .expect("Canonicalize path");

    // We need to write the PSF path into the config files.
    let psf_conf = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // Copy the tgen client config. We keep the template suffix because we only
    // partially instantiate it here and the rest happens during the sim.
    {
        let conf = "tgen-client.graphml.xml.template";
        let in_path = PathBuf::from(format!("{}/{conf}", test_dir_in.display()));
        let out_path = PathBuf::from(format!("{}/{conf}", test_dir_out.display()));
        let replacements = vec![("${PSFPATH}", psf_conf.to_str().unwrap())];
        copy_test_file_with_replace(&in_path, &out_path, replacements);
    }

    // Copy the shadow config, instantiating the template variables.
    {
        let in_path = PathBuf::from(format!("{}/shadow.yaml.template", test_dir_in.display()));
        let out_path = PathBuf::from(format!("{}/shadow.yaml", test_dir_out.display()));
        let replacements = vec![
            ("${TGENSERVERCONF}", tgen_server_conf.to_str().unwrap()),
            ("${PSFPATH}", psf_conf.to_str().unwrap()),
            ("${PROTEUSBINPATH}", bin_path.to_str().unwrap()),
        ];
        copy_test_file_with_replace(&in_path, &out_path, replacements);
    }

    test_dir_out
}

fn copy_test_file_with_replace(src: &PathBuf, dst: &PathBuf, replacements: Vec<(&str, &str)>) {
    // fs::copy(src, dst).expect("Copy test file");
    let mut contents = fs::read_to_string(src).expect("Read test file contents");
    for (from, to) in replacements {
        contents = contents.replace(from, to);
    }
    fs::write(dst, contents).expect("Write test file contents");
}

fn count_stream_successes(tgen_log_file: PathBuf) -> u64 {
    let mut count = 0;
    let log = File::open(tgen_log_file).expect("TGen log file");
    for line in BufReader::new(log).lines() {
        if line.unwrap().contains("[stream-success]") {
            count += 1;
        }
    }
    count
}
