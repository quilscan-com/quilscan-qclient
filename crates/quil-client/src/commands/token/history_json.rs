//! Opt-in machine projection of the native local wallet action history.

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::{action_log::ActionLog, history};

pub(super) const MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Serialize)]
struct Envelope<'a> {
    schema_version: u8,
    scope: &'static str,
    config_dir: &'a str,
    recording_enabled: bool,
    file_present: bool,
    limit: usize,
    total_actions: usize,
    unreadable_lines: usize,
    actions: Vec<Action<'a>>,
}

#[derive(Serialize)]
struct Action<'a> {
    kind: &'a str,
    source: Option<&'a str>,
    at: String,
    started: bool,
    outcome: &'a str,
    amount_base_units: Option<String>,
    amount_quil: Option<String>,
    fee_base_units: Option<String>,
    fee_quil: Option<String>,
    party: Option<Party<'a>>,
    batches: Option<Batches>,
    duration_s: Option<String>,
    error: Option<&'a str>,
}

#[derive(Serialize)]
struct Party<'a> {
    kind: &'static str,
    value: &'a str,
}

#[derive(Serialize)]
struct Batches {
    submitted: String,
    planned: Option<String>,
}

impl<'a> From<&'a history::Action> for Action<'a> {
    fn from(action: &'a history::Action) -> Self {
        Self {
            kind: &action.kind,
            source: action.source.as_deref(),
            at: action.at.to_string(),
            started: action.started,
            outcome: action.outcome.word(),
            amount_base_units: action.amount.map(|amount| amount.to_string()),
            amount_quil: action.amount.map(history::quil),
            fee_base_units: action.fee.map(|fee| fee.to_string()),
            fee_quil: action.fee.map(history::quil),
            party: action.party.as_ref().map(|party| match party {
                history::Party::Recipient(value) => Party {
                    kind: "recipient",
                    value,
                },
                history::Party::Escrow(value) => Party {
                    kind: "escrow",
                    value,
                },
            }),
            batches: action.batches.map(|(submitted, planned)| Batches {
                submitted: submitted.to_string(),
                planned: planned.map(|count| count.to_string()),
            }),
            duration_s: action.duration_s.map(|seconds| seconds.to_string()),
            error: action.error.as_deref(),
        }
    }
}

/// Serialize into private bounded memory first, reserving the final newline.
/// A failed read or serialization never writes part of a document to stdout.
#[derive(Default)]
struct DocumentBuffer(Vec<u8>);

impl Write for DocumentBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_OUTPUT_BYTES - 1 - self.0.len() {
            return Err(std::io::Error::other(format!(
                "token history JSON exceeds {MAX_OUTPUT_BYTES} bytes; use a smaller --limit"
            )));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn document(
    config_dir: &Path,
    limit: usize,
    recording_enabled: bool,
) -> anyhow::Result<Vec<u8>> {
    let identity = config_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("selected config directory is not valid UTF-8"))?;
    let text = ActionLog::new(config_dir).read()?;
    let parsed = history::parse(text.as_deref().unwrap_or_default());
    let shown = if limit == 0 {
        parsed.actions.len()
    } else {
        limit.min(parsed.actions.len())
    };
    let envelope = Envelope {
        schema_version: 1,
        scope: "local_wallet_action_log",
        config_dir: identity,
        recording_enabled,
        file_present: text.is_some(),
        limit,
        total_actions: parsed.actions.len(),
        unreadable_lines: parsed.unreadable,
        actions: parsed.actions[..shown].iter().map(Action::from).collect(),
    };
    let mut buffer = DocumentBuffer::default();
    serde_json::to_writer(&mut buffer, &envelope)?;
    buffer.0.push(b'\n');
    Ok(buffer.0)
}

pub(super) fn run(config_dir: &Path, limit: usize) -> anyhow::Result<()> {
    let bytes = document(config_dir, limit, !crate::config::action_log_off())?;
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn log(config: &Path, text: impl AsRef<[u8]>) -> std::path::PathBuf {
        let dir = config.join("wallet");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let file = dir.join("actions.jsonl");
        std::fs::write(&file, text).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        file
    }

    fn value(config: &Path, limit: usize, recording_enabled: bool) -> Value {
        serde_json::from_slice(&document(config, limit, recording_enabled).unwrap()).unwrap()
    }

    #[test]
    fn missing_and_empty_logs_have_distinct_presence_without_creating_files() {
        let config = tempfile::tempdir().unwrap();
        let missing = value(config.path(), 20, false);
        assert_eq!(missing["schema_version"], 1);
        assert_eq!(missing["scope"], "local_wallet_action_log");
        assert_eq!(missing["recording_enabled"], false);
        assert_eq!(missing["file_present"], false);
        assert_eq!(missing["actions"], json!([]));
        assert_eq!(missing["total_actions"], 0);
        assert_eq!(missing["unreadable_lines"], 0);
        assert!(!config.path().join("wallet").exists());
        log(config.path(), "");
        let empty = value(config.path(), 0, true);
        assert_eq!(empty["file_present"], true);
        assert_eq!(empty["recording_enabled"], true);
        assert_eq!(empty["actions"], json!([]));
        assert_eq!(empty["limit"], 0);
    }

    #[test]
    fn projection_reuses_native_grouping_and_keeps_exact_strings_and_nulls() {
        let config = tempfile::tempdir().unwrap();
        let text = [
            json!({"ts":u64::MAX,"kind":"transfer","phase":"start","source":"cli","amount":u128::MAX.to_string(),"fee":"0","recipient":"<recipient>"}).to_string(),
            json!({"ts":u64::MAX,"kind":"transfer","phase":"outcome","source":"cli","outcome":"future","duration_s":u64::MAX,"error":"<error>"}).to_string(),
            json!({"ts":0,"kind":"mint_claim","phase":"outcome","outcome":"ok","minted":"0","escrow":"escrow","duration_s":0}).to_string(),
            json!({"ts":1,"kind":"pending_transfer","phase":"start","source":"manager"}).to_string(),
            "bad line".into(),
            "[]".into(),
        ].join("\n");
        let path = log(config.path(), &text);
        let before = std::fs::read(&path).unwrap();
        let output = value(config.path(), 0, true);
        assert_eq!(output.as_object().unwrap().len(), 9);
        assert_eq!(output["unreadable_lines"], 2);
        assert_eq!(output["total_actions"], 3);
        let native = history::parse(&text);
        let rows = output["actions"].as_array().unwrap();
        for (row, action) in rows.iter().zip(&native.actions) {
            assert_eq!(row.as_object().unwrap().len(), 13);
            assert_eq!(row["kind"], action.kind);
            assert_eq!(row["at"], action.at.to_string());
            assert_eq!(row["outcome"], action.outcome.word());
        }
        let first = &rows[0];
        assert_eq!(first["amount_base_units"], u128::MAX.to_string());
        assert_eq!(first["amount_quil"], history::quil(u128::MAX));
        assert_eq!(first["fee_base_units"], "0");
        assert_eq!(first["fee_quil"], "0");
        assert_eq!(first["duration_s"], u64::MAX.to_string());
        assert_eq!(
            first["party"],
            json!({"kind":"recipient","value":"<recipient>"})
        );
        assert!(first["batches"].is_null());
        assert_eq!(rows[1]["outcome"], "no outcome");
        assert!(rows[1]["amount_base_units"].is_null());
        assert!(rows[1]["amount_quil"].is_null());
        let last = &rows[2];
        assert_eq!(last["kind"], "mint_claim");
        assert_eq!(last["at"], "0");
        assert_eq!(last["started"], false);
        assert!(last["source"].is_null());
        assert_eq!(last["amount_base_units"], "0");
        assert!(last["fee_base_units"].is_null());
        assert_eq!(last["duration_s"], "0");
        assert_eq!(last["party"], json!({"kind":"escrow","value":"escrow"}));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn shield_batch_counts_and_unknown_outcomes_keep_native_meaning() {
        let config = tempfile::tempdir().unwrap();
        let text = [
            json!({"ts":1,"kind":"shield","phase":"start","source":"manager","batches":u64::MAX}),
            json!({"ts":2,"kind":"shield","phase":"submitted","source":"manager","shielded":"8000000001","fee":"1"}),
            json!({"ts":3,"kind":"shield","phase":"submitted","source":"manager","shielded":"2","fee":"2"}),
            json!({"ts":4,"kind":"shield","phase":"outcome","source":"manager","outcome":"unknown outcome","duration_s":0}),
        ].map(|line|line.to_string()).join("\n");
        log(config.path(), text);
        let output = value(config.path(), 20, true);
        let row = &output["actions"][0];
        assert_eq!(row["amount_base_units"], "8000000003");
        assert_eq!(row["amount_quil"], "1.000000000375");
        assert_eq!(row["fee_base_units"], "3");
        assert_eq!(
            row["batches"],
            json!({"submitted":"2","planned":u64::MAX.to_string()})
        );
        assert_eq!(row["outcome"], "unknown outcome");
    }

    #[test]
    fn limits_apply_after_native_ordering_and_preserve_total_actions() {
        let config = tempfile::tempdir().unwrap();
        log(
            config.path(),
            (0..25)
                .map(|n| {
                    json!({"ts":n,"kind":"merge","phase":"outcome","outcome":"ok"}).to_string()
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
        for (limit, shown) in [(20, 20), (0, 25), (2, 2), (100, 25)] {
            let output = value(config.path(), limit, true);
            assert_eq!(output["limit"], limit);
            assert_eq!(output["total_actions"], 25);
            assert_eq!(output["actions"].as_array().unwrap().len(), shown);
            assert_eq!(output["actions"][0]["at"], "24");
        }
    }

    #[test]
    fn explicit_path_identity_is_retained_even_when_the_config_path_is_an_alias() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node");
        std::fs::create_dir(&config).unwrap();
        let alias = dir.path().join("alias");
        symlink(&config, &alias).unwrap();
        assert_eq!(
            value(&alias, 20, true)["config_dir"],
            alias.to_str().unwrap()
        );
    }

    #[test]
    fn native_reader_rejects_unsafe_modes_links_and_non_utf8_data() {
        let config = tempfile::tempdir().unwrap();
        let file = log(config.path(), "");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(document(config.path(), 20, true)
            .unwrap_err()
            .to_string()
            .contains("owner only"));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let target = config.path().join("elsewhere");
        std::fs::rename(&file, &target).unwrap();
        symlink(&target, &file).unwrap();
        assert!(document(config.path(), 20, true)
            .unwrap_err()
            .to_string()
            .contains("not a regular file"));
        std::fs::remove_file(&file).unwrap();
        log(config.path(), [0xff]);
        assert!(document(config.path(), 20, true).is_err());
    }

    #[test]
    fn serialized_budget_counts_escaping_and_rejects_the_entire_document() {
        let config = tempfile::tempdir().unwrap();
        log(config.path(),json!({"ts":1,"kind":"transfer","phase":"outcome","outcome":"failed","error":"\u{1}".repeat(90_000)}).to_string());
        assert!(document(config.path(), 0, true)
            .unwrap_err()
            .to_string()
            .contains("524288"));
        log(config.path(),(0..2000).map(|n|json!({"ts":n,"kind":"transfer","phase":"outcome","outcome":"failed","error":"x".repeat(250)}).to_string()).collect::<Vec<_>>().join("\n"));
        assert!(document(config.path(), 0, true).is_err());
        assert!(document(config.path(), 20, true).unwrap().len() <= MAX_OUTPUT_BYTES);
    }
}
