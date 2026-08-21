//! Persistent, source-sensitive acknowledgement of rejected LLM findings.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::analysis::findings::Finding;
use crate::diff::hunks::Hunk;

pub const DEFAULT_PATH: &str = ".drep/acknowledgements.toml";
const CONTEXT_RADIUS: u32 = 3;

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    #[serde(default)]
    fingerprints: BTreeSet<String>,
}

impl Store {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(DEFAULT_PATH);
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(err).with_context(|| format!("could not read {}", path.display()));
            }
        };
        toml::from_str(&raw).with_context(|| format!("could not parse {}", path.display()))
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.fingerprints.contains(fingerprint)
    }

    pub fn insert(&mut self, fingerprint: String) -> bool {
        self.fingerprints.insert(fingerprint)
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = root.join(DEFAULT_PATH);
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
        let rendered = toml::to_string_pretty(self).context("could not render acknowledgements")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
            format!("could not create a temporary file in {}", parent.display())
        })?;
        temporary
            .write_all(rendered.as_bytes())
            .and_then(|()| temporary.flush())
            .and_then(|()| temporary.as_file().sync_all())
            .with_context(|| format!("could not write {}", path.display()))?;
        temporary
            .persist(&path)
            .map_err(|err| err.error)
            .with_context(|| format!("could not publish {}", path.display()))?;
        Ok(())
    }
}

pub fn validate_fingerprint(value: &str) -> Result<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid finding fingerprint `{value}`; expected 64 lowercase hexadecimal characters"
        ))
    }
}

/// Attach source-sensitive fingerprints and remove ones already adjudicated.
pub fn apply(findings: &mut Vec<Finding>, by_file: &[Vec<Hunk>], store: &Store) {
    let hunks_by_file: std::collections::BTreeMap<&Path, &[Hunk]> = by_file
        .iter()
        .filter_map(|hunks| {
            hunks
                .first()
                .map(|first| (first.file_path.as_path(), hunks.as_slice()))
        })
        .collect();
    for finding in findings.iter_mut() {
        finding.fingerprint = hunks_by_file
            .get(Path::new(&finding.file_path))
            .and_then(|hunks| fingerprint(finding, hunks));
    }
    findings.retain(|finding| {
        finding
            .fingerprint
            .as_deref()
            .is_none_or(|fingerprint| !store.contains(fingerprint))
    });
}

fn fingerprint(finding: &Finding, hunks: &[Hunk]) -> Option<String> {
    let start = finding.line.saturating_sub(CONTEXT_RADIUS);
    let end = finding.line.saturating_add(CONTEXT_RADIUS);
    let mut context = Vec::new();
    let mut contains_target = false;
    for (number, content) in hunks.iter().flat_map(Hunk::numbered_new_lines) {
        if number == finding.line {
            contains_target = true;
        }
        if (start..=end).contains(&number) {
            context.push((number == finding.line, content));
        }
    }
    if !contains_target {
        return None;
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"drep-acknowledgement-v1\0");
    hasher.update(finding.file_path.as_bytes());
    hasher.update(b"\0");
    hasher.update(finding.kind.as_bytes());
    for (is_target, line) in context {
        hasher.update(b"\0");
        hasher.update(if is_target { b"target\0" } else { b"context\0" });
        hasher.update(line.as_bytes());
    }
    Some(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::findings::Severity;
    use std::path::PathBuf;

    fn finding(line: u32) -> Finding {
        Finding {
            kind: "bug".to_owned(),
            severity: Severity::Error,
            file_path: "src/lib.rs".to_owned(),
            line,
            column: None,
            message: "message".to_owned(),
            suggestion: None,
            asserts_compile_failure: false,
            fingerprint: None,
        }
    }

    #[test]
    fn fingerprint_survives_line_movement_but_expires_on_source_change() {
        let original = Hunk::whole_file(
            PathBuf::from("src/lib.rs"),
            "zero\na\nb\nc\ntarget\nd\ne\nf\ntail\n",
        );
        let shifted = Hunk::whole_file(
            PathBuf::from("src/lib.rs"),
            "extra\nzero\na\nb\nc\ntarget\nd\ne\nf\ntail\n",
        );
        let changed = Hunk::whole_file(
            PathBuf::from("src/lib.rs"),
            "zero\na\nb\nc\ntarget changed\nd\ne\nf\ntail\n",
        );
        let first = fingerprint(&finding(5), &[original]).expect("first fingerprint");
        assert_eq!(first, fingerprint(&finding(6), &[shifted]).unwrap());
        assert_ne!(first, fingerprint(&finding(5), &[changed]).unwrap());
    }

    #[test]
    fn store_round_trips_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = Store::default();
        let key = "a".repeat(64);
        assert!(store.insert(key.clone()));
        store.save(dir.path()).expect("save");
        assert!(Store::load(dir.path()).expect("load").contains(&key));
    }

    #[test]
    fn applying_a_recorded_fingerprint_suppresses_the_same_source_context() {
        let hunks = vec![vec![Hunk::whole_file(
            PathBuf::from("src/lib.rs"),
            "one\ntarget\nthree\n",
        )]];
        let mut first = vec![finding(2)];
        apply(&mut first, &hunks, &Store::default());
        let key = first[0].fingerprint.clone().expect("fingerprint");

        let mut store = Store::default();
        store.insert(key);
        let mut repeated = vec![finding(2)];
        apply(&mut repeated, &hunks, &store);

        assert!(repeated.is_empty());
    }

    #[test]
    fn two_findings_in_the_same_context_have_distinct_fingerprints() {
        let hunk = Hunk::whole_file(PathBuf::from("src/lib.rs"), "one\ntwo\nthree\n");
        assert_ne!(
            fingerprint(&finding(1), std::slice::from_ref(&hunk)),
            fingerprint(&finding(2), std::slice::from_ref(&hunk))
        );
    }
}
