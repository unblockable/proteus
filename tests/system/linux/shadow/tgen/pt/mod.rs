use std::fs;
use std::path::{Path, PathBuf};

mod direct;
mod persist;

fn initialize_test_directory(rel_test_src: &PathBuf, psf_filepath: &Path, persist: &str) -> PathBuf {
    // Set up our paths
    let test_name = psf_filepath.file_stem().unwrap();
    let rel_test_src_parent = PathBuf::from(rel_test_src.parent().unwrap());
    let rel_test_src_gparent = rel_test_src_parent.parent().unwrap();
    let rel_test_dst = PathBuf::from("target").join(rel_test_src).join(test_name);

    // We need to write the proteus bin and PSF paths into the config files.
    let abs_bin = PathBuf::from(test_bin::get_test_bin!("proteus").get_program());
    let abs_psf = fs::canonicalize(psf_filepath).expect("Canonicalize path");

    // The tgen server conf does not change, so just use the static one.
    let abs_tgen_server_conf = fs::canonicalize(rel_test_src_gparent.join("tgen-server.graphml"))
        .expect("Canonicalize path");

    // Shadow needs a clear working directory.
    super::super::remove_and_create_all(&rel_test_dst);

    // Copy the tgen client config. We keep the template suffix because we only
    // partially instantiate it here and the rest happens during the sim.
    {
        let conf = "tgen-client.graphml.template";
        let src = rel_test_src_parent.join(conf);
        let dst = rel_test_dst.join(conf);
        let replacements = vec![
            ("${PSFPATH}", abs_psf.to_str().unwrap()),
            ("${PERSIST}", persist),
        ];
        super::super::copy_test_file_with_replace(&src, &dst, replacements);
    }

    // Copy the shadow config, instantiating the template variables.
    {
        let src = rel_test_src_parent.join("shadow.yaml.template");
        let dst = rel_test_dst.join("shadow.yaml");
        let replacements = vec![
            ("${TGENSERVERCONF}", abs_tgen_server_conf.to_str().unwrap()),
            ("${PSFPATH}", abs_psf.to_str().unwrap()),
            ("${PROTEUSBINPATH}", abs_bin.to_str().unwrap()),
            ("${PERSIST}", persist),
        ];
        super::super::copy_test_file_with_replace(&src, &dst, replacements);
    }

    rel_test_dst
}
