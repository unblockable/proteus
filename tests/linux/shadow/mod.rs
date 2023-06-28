use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

#[cfg(all(target_os = "linux", have_shadow, have_tgen))]
mod tgen;
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor))]
mod tor;

fn run_shadow<I, S>(run_dir: &PathBuf, args: I) -> ExitStatus
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let log = File::create(format!("{}/shadow.log", run_dir.display())).expect("Create log file");
    let mut cmd = Command::new("shadow")
        .current_dir(run_dir.clone())
        .args(args)
        .stdout(Stdio::from(log.try_clone().expect("Clone log")))
        .stderr(Stdio::from(log))
        .spawn()
        .expect("Starting shadow process");
    cmd.wait().expect("Shadow not running")
}

fn remove_and_create_all(dir: PathBuf) {
    if dir.exists() {
        fs::remove_dir_all(dir.clone()).expect("Remove test output dir");
    }
    fs::create_dir_all(dir).expect("Create test output dir");
}

fn copy_test_file_with_replace(src: &PathBuf, dst: &PathBuf, replacements: Vec<(&str, &str)>) {
    // fs::copy(src, dst).expect("Copy test file");
    let mut contents = fs::read_to_string(src).expect("Read test file contents");
    for (from, to) in replacements {
        contents = contents.replace(from, to);
    }
    fs::write(dst, contents).expect("Write test file contents");
}

fn count_tgen_stream_successes(tgen_log_file: PathBuf) -> u64 {
    let mut count = 0;
    let log = File::open(tgen_log_file).expect("TGen log file");
    for line in BufReader::new(log).lines() {
        if line.unwrap().contains("[stream-success]") {
            count += 1;
        }
    }
    count
}
