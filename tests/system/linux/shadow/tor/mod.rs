use std::fs;
use std::path::{Path, PathBuf};

mod direct;
mod persist;

fn initialize_test_directory(
    rel_test_src: &PathBuf,
    psf_filepath: &Path,
    persist: &str,
) -> (PathBuf, PathBuf) {
    // Set up the dir from which we will run the test.
    let test_name = psf_filepath.file_stem().unwrap();
    let rel_test_src_parent = PathBuf::from(rel_test_src.parent().unwrap());
    let rel_test_dst = PathBuf::from("target").join(&rel_test_src).join(test_name);

    // We need to write the proteus bin and PSF paths into the config files.
    let abs_bin = PathBuf::from(test_bin::get_test_bin!("proteus").get_program());
    let abs_psf = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // Shadow needs a clear working directory.
    super::remove_and_create_all(&rel_test_dst);

    // Copy the tor config. We keep the template suffix because we only
    // partially instantiate it here and the rest happens during the sim.
    {
        let conf = "torrc.template";
        let src = rel_test_src_parent.join(conf);
        let dst = rel_test_dst.join(conf);
        let replacements = vec![
            ("${PSFPATH}", abs_psf.to_str().unwrap()),
            ("${PERSIST}", persist),
        ];
        super::copy_test_file_with_replace(&src, &dst, replacements);
    }

    // Copy the shadow config, instantiating the template variables.
    {
        let src = rel_test_src_parent.join("shadow.yaml.template");
        let dst = rel_test_dst.join("shadow.yaml");
        let replacements = vec![
            ("${PSFPATH}", abs_psf.to_str().unwrap()),
            ("${PROTEUSBINPATH}", abs_bin.to_str().unwrap()),
            ("${PERSIST}", persist),
        ];
        super::copy_test_file_with_replace(&src, &dst, replacements);
    }

    (rel_test_dst, rel_test_src_parent)
}
