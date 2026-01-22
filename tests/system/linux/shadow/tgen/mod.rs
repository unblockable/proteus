use std::fs;
use std::path::{Path, PathBuf};

mod pt;
mod stream;
mod tunnel;
mod turbo_stream;
mod turbo_tunnel;

fn initialize_test_directory(
    rel_test_src: &PathBuf,
    psf_filepath: &Path,
    turbo: &str,
    mode: &str,
) -> PathBuf {
    // Set up the dir from which we will run the test.
    let test_name = psf_filepath.file_stem().unwrap();
    let rel_test_src_parent = PathBuf::from(rel_test_src.parent().unwrap());
    let rel_test_dst = PathBuf::from("target").join(rel_test_src).join(test_name);

    // We need to write the proteus bin and PSF paths into the config files.
    let abs_bin = PathBuf::from(test_bin::get_test_bin!("proteus").get_program());
    let abs_psf = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // Use the common tgen confs.
    let abs_tgen_server_conf = fs::canonicalize(rel_test_src_parent.join("tgen-server.graphml"))
        .expect("Canonicalize path");
    let abs_tgen_client_conf = fs::canonicalize(rel_test_src_parent.join("tgen-client.graphml"))
        .expect("Canonicalize path");

    // Shadow needs a clear working directory.
    super::remove_and_create_all(&rel_test_dst);

    // Copy the shadow config, instantiating the template variables.
    {
        let src = rel_test_src_parent.join("shadow.yaml.template");
        let dst = rel_test_dst.join("shadow.yaml");
        let replacements = vec![
            ("${TGENSERVERCONF}", abs_tgen_server_conf.to_str().unwrap()),
            ("${TGENCLIENTCONF}", abs_tgen_client_conf.to_str().unwrap()),
            ("${PSFPATH}", abs_psf.to_str().unwrap()),
            ("${PROTEUSBINPATH}", abs_bin.to_str().unwrap()),
            ("${TURBO}", turbo),
            ("${MODE}", mode),
        ];
        super::copy_test_file_with_replace(&src, &dst, replacements);
    }

    rel_test_dst
}
