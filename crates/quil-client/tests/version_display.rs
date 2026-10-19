//! Operator-facing versions follow official source metadata; Agent release
//! identity remains the Quilscan release manifest's responsibility.

use std::process::{Command, Output};

fn expected_output() -> String {
    let subpatch = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../subpatch"))
        .unwrap_or_default();
    let subpatch = subpatch.trim();
    let version = quil_config::VERSION_STRING;
    if subpatch.is_empty() {
        format!("qclient {version}\n")
    } else {
        format!("qclient {version}.{subpatch}\n")
    }
}

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_qclient"))
        .args(["--network", "__version_does_not_need_network_config__"])
        .args(args)
        .output()
        .expect("run qclient")
}

#[test]
fn version_flag_reports_source_version_without_loading_network() {
    let output = invoke(&["--version"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected_output());
}

#[test]
fn version_command_matches_official_version_flag() {
    let output = invoke(&["version"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected_output());
}

#[test]
fn version_checksum_preserves_integrity_hashes_after_version_sync() {
    use sha2::{Digest, Sha256};

    let output = invoke(&["version", "--checksum"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output);
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_qclient")).unwrap();
    let expected = format!(
        "{}SHA256: {}\nMD5: {}\n",
        expected_output(),
        hex::encode(Sha256::digest(&bytes)),
        hex::encode(md5::Md5::digest(&bytes)),
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}
