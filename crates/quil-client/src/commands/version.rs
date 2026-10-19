//! Compatibility alias for the official `qclient --version` display.
//! `--checksum` additionally prints SHA-256 + MD5 of the running binary.

use clap::Args;
use quil_config::display_version;

#[derive(Debug, Args)]
pub struct VersionArgs {
    /// Show the SHA-256 and MD5 checksums of the running binary.
    #[arg(short = 'c', long = "checksum")]
    pub checksum: bool,
}

pub fn run(args: &VersionArgs) -> anyhow::Result<()> {
    println!("qclient {}", display_version());

    if args.checksum {
        let exe = std::env::current_exe()?;
        let bytes = std::fs::read(&exe)?;
        println!("SHA256: {}", sha256_hex(&bytes));
        println!("MD5: {}", md5_hex(&bytes));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn md5_hex(bytes: &[u8]) -> String {
    // MD5 is a non-protocol integrity hash used only for the version
    // display (matches Go `CalculateFileHashes`).
    use md5::{Digest, Md5};
    hex::encode(Md5::digest(bytes))
}
