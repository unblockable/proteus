use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

#[cfg(all(target_os = "linux", have_shadow, have_tgen))]
mod tgen;
#[cfg(all(target_os = "linux", have_shadow, have_tgen, have_tor))]
mod tor;

fn run_shadow_and_assert_result(
    run_dir: &Path,
    template_dir: Option<PathBuf>,
    tgen_pid_server: u32,
    tgen_pid_client: u32,
) {
    let mut run_args = vec![];

    if let Some(path) = template_dir.as_ref() {
        let template_arg = format!("--template-directory={}", path.display());
        run_args.push(template_arg);
    }
    run_args.push("--parallelism=4".into());
    // We disable CPU pinning because we are running many Shadow sims at the same time and we
    // don't want them all to pin to the same set of CPUs.
    run_args.push("--use-cpu-pinning=false".into());
    run_args.push("shadow.yaml".into());

    assert!(run_shadow(run_dir, run_args).success());

    let client = run_dir
        .join("shadow.data")
        .join("hosts")
        .join("client")
        .join(format!("tgen.{tgen_pid_client}.stdout"));
    let server = run_dir
        .join("shadow.data")
        .join("hosts")
        .join("server")
        .join(format!("tgen.{tgen_pid_server}.stdout"));

    assert_eq!(count_tgen_stream_successes(&client), 5);
    assert_eq!(count_tgen_stream_successes(&server), 5);
}

fn run_shadow<I, S>(run_dir: &Path, args: I) -> ExitStatus
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let log = File::create(format!("{}/shadow.log", run_dir.display())).expect("Create log file");
    let mut cmd = Command::new("shadow")
        .current_dir(run_dir)
        .args(args)
        .stdout(Stdio::from(log.try_clone().expect("Clone log")))
        .stderr(Stdio::from(log))
        .spawn()
        .expect("Starting shadow process");
    cmd.wait().expect("Shadow not running")
}

fn remove_and_create_all(dir: &PathBuf) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("Remove test output dir");
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

fn count_tgen_stream_successes(tgen_log_file: &PathBuf) -> u64 {
    let mut count = 0;
    let log = File::open(tgen_log_file).expect("TGen log file");
    for line in BufReader::new(log).lines() {
        if line.unwrap().contains("[stream-success]") {
            count += 1;
        }
    }
    count
}
