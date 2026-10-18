//! `sign <secret-key-file> <file>`: writes `<file>.minisig` (minisign format).
//! The key password comes from MINISIGN_PASSWORD. Used by the release workflow;
//! the secret key never leaves CI.

use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: sign <secret-key-file> <file>".into());
    }
    let sk = minisign::SecretKeyBox::from_string(&std::fs::read_to_string(&args[1])?)?.into_secret_key(Some(std::env::var("MINISIGN_PASSWORD")?))?;
    let sig = minisign::sign(None, &sk, File::open(&args[2])?, None, Some("lindesk release"))?;
    std::fs::write(format!("{}.minisig", args[2]), sig.to_string())?;
    Ok(())
}
