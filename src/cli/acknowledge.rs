//! Record source-sensitive LLM findings that have been reviewed and rejected.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::Exit;
use crate::analysis::acknowledgements::{Store, validate_fingerprint};

#[derive(Debug, Args)]
pub struct AcknowledgeArgs {
    /// Finding fingerprints printed by `drep check`.
    #[arg(required = true, value_name = "FINGERPRINT")]
    pub fingerprints: Vec<String>,
}

pub fn run(args: &AcknowledgeArgs, root: &Path) -> Result<Exit> {
    for fingerprint in &args.fingerprints {
        validate_fingerprint(fingerprint)?;
    }
    let mut store = Store::load(root)?;
    let added = args
        .fingerprints
        .iter()
        .filter(|fingerprint| store.insert((*fingerprint).clone()))
        .count();
    store.save(root)?;
    println!(
        "Acknowledged {added} new finding(s) in {}.",
        root.join(crate::analysis::acknowledgements::DEFAULT_PATH)
            .display()
    );
    Ok(Exit::Clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_short_or_uppercase_fingerprint_without_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        for value in ["abc".to_owned(), "A".repeat(64)] {
            let error = run(
                &AcknowledgeArgs {
                    fingerprints: vec![value],
                },
                dir.path(),
            )
            .expect_err("invalid fingerprint");
            assert!(error.to_string().contains("64 lowercase hexadecimal"));
        }
        assert!(
            !dir.path()
                .join(crate::analysis::acknowledgements::DEFAULT_PATH)
                .exists()
        );
    }
}
