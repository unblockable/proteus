mod integration {
    use std::path::Path;

    use test_each_file::test_each_path;

    fn run_proteus_check_test([psf_filepath]: [&Path; 1]) {
        for len in [
            1, 10, 100, 1000, 1500, 2000, 5000, 10_000, 100_000, 1_000_000,
        ] {
            let output = test_bin::get_test_bin("proteus")
                .arg("check")
                .args(["--num-bytes", format!("{len}").as_str()])
                .arg(psf_filepath)
                .output()
                .expect("Failed to start proteus");

            assert!(output.status.success())
        }
    }

    test_each_path! {for ["psf"] in "tests/fixtures" => run_proteus_check_test}
}
