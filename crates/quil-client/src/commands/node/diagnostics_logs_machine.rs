//! Noninteractive owner operations. A preview freezes ciphertext, never plaintext.
//! The filesystem capability is a random token, bound to the exact queued protobuf.
use super::*;
use prost::Message as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read as _;

const MAX_JSON: usize = 128 * 1024;
const MAX_MAP: usize = 16 * 1024 * 1024;
const MAX_META: usize = 16 * 1024;
const STAGE_TTL_MS: u64 = 10 * 60 * 1000;
const MAX_STAGES: usize = 4;

#[derive(Debug, Subcommand)]
pub enum MachineCommand {
    /// List the authenticated owner's current Node queue as JSON.
    List,
    /// Freeze a pending request's redacted, encrypted export without sending it.
    Prepare {
        #[arg(value_parser = request_id)]
        request_id: String,
    },
    /// Send exactly the frozen ciphertext once, after a fresh queue check.
    Approve {
        #[arg(value_parser = request_id)]
        request_id: String,
        #[arg(long, value_parser = token)]
        token: String,
    },
    /// Deny a request or withdraw an existing bundle.
    Deny {
        #[arg(value_parser = request_id)]
        request_id: String,
    },
    /// Delete only a local preparation, leaving the Node queue unchanged.
    Discard {
        #[arg(value_parser = request_id)]
        request_id: String,
        #[arg(long, value_parser = token)]
        token: String,
    },
}
pub(crate) fn config_dir(text: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(text);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("--config-dir must be an absolute Node configuration directory".into())
    }
}
fn load_selected_config(
    ctx: &Context,
    dir: &Path,
) -> anyhow::Result<(quil_config::Config, PathBuf)> {
    anyhow::ensure!(
        dir.is_absolute() && dir.join("config.yml").is_file(),
        "selected Node configuration does not exist"
    );
    let path = dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("selected Node configuration path is not UTF-8"))?;
    ctx.load_node_config(path)
}
fn exact_hex(text: &str, length: usize) -> Result<String, String> {
    if text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(text.into())
    } else {
        Err(format!("expected {length} lowercase hex digits"))
    }
}
fn request_id(text: &str) -> Result<String, String> {
    exact_hex(text, 32)
}
fn token(text: &str) -> Result<String, String> {
    exact_hex(text, 64)
}
fn status_name(request: &PendingLogRequest) -> &'static str {
    match request.status() {
        LogRequestStatus::Pending => "pending",
        LogRequestStatus::Ready => "ready",
        LogRequestStatus::Served => "served",
        _ => "unknown",
    }
}
#[derive(Serialize)]
struct Request {
    request_id: String,
    requester_peer_id: String,
    status: &'static str,
    received_unix_ms: u64,
    expires_unix_ms: u64,
    since_unix: u64,
    until_unix: u64,
    filters: Vec<String>,
    bundle_bytes: u64,
}
impl Request {
    fn from_proto(r: &PendingLogRequest) -> anyhow::Result<Self> {
        anyhow::ensure!(
            r.request_id.len() == logs::ID_LEN,
            "Node returned an invalid request ID"
        );
        let peer = logs::peer_from_bytes(&r.requester_peer_id)
            .ok_or_else(|| anyhow::anyhow!("Node returned an invalid requester identity"))?;
        anyhow::ensure!(
            r.filters.len() <= logs::MAX_FILTERS
                && r.filters
                    .iter()
                    .all(|f| logs::SHARD_FILTER_BYTES.contains(&f.len())),
            "Node returned invalid shard filters"
        );
        anyhow::ensure!(
            r.since_unix < r.until_unix
                && r.until_unix - r.since_unix <= logs::MAX_WINDOW_SECS
                && r.received_unix_ms < r.expires_unix_ms
                && r.bundle_bytes <= MAX_FETCH_BYTES,
            "Node returned invalid request bounds"
        );
        Ok(Self {
            request_id: hex::encode(&r.request_id),
            requester_peer_id: peer.to_base58(),
            status: status_name(r),
            received_unix_ms: r.received_unix_ms,
            expires_unix_ms: r.expires_unix_ms,
            since_unix: r.since_unix,
            until_unix: r.until_unix,
            filters: r.filters.iter().map(hex::encode).collect(),
            bundle_bytes: r.bundle_bytes,
        })
    }
}
#[derive(Serialize)]
struct List {
    schema_version: u32,
    requests: Vec<Request>,
    max_bundle_bytes: u64,
}
fn list_dto(listed: &ListLogRequestsResponse) -> anyhow::Result<List> {
    anyhow::ensure!(
        listed.requests.len() <= 16,
        "Node returned too many log requests"
    );
    check_max(listed.max_bundle_bytes)?;
    Ok(List {
        schema_version: 1,
        requests: listed
            .requests
            .iter()
            .map(Request::from_proto)
            .collect::<anyhow::Result<_>>()?,
        max_bundle_bytes: listed.max_bundle_bytes,
    })
}
#[derive(Serialize)]
struct Preview {
    schema_version: u32,
    request: Request,
    prepared_token: String,
    expires_unix_ms: u64,
    sealed_bytes: u64,
    preview: PreviewDetails,
}
#[derive(Serialize)]
struct PreviewDetails {
    lines_read: u64,
    lines_kept: u64,
    source_kind: String,
    source_count: u64,
    redactions: BTreeMap<String, u64>,
    redacted_samples: Vec<String>,
    samples_truncated: bool,
    unreadable_sources: u64,
    note: String,
}
#[derive(Serialize)]
struct Approved {
    schema_version: u32,
    request_id: String,
    outcome: &'static str,
    bundle_bytes: u64,
    expires_unix_ms: u64,
}
#[derive(Serialize)]
struct Denied {
    schema_version: u32,
    request_id: String,
    outcome: &'static str,
    previous_status: String,
}
#[derive(Serialize)]
struct Discarded {
    schema_version: u32,
    request_id: String,
    outcome: &'static str,
}
fn json_bytes<T: Serialize>(value: &T) -> anyhow::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    anyhow::ensure!(bytes.len() < MAX_JSON, "machine JSON exceeds 128 KiB");
    Ok(bytes)
}
fn emit<T: Serialize>(value: &T) -> anyhow::Result<()> {
    let bytes = json_bytes(value)?;
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)?;
    out.write_all(b"\n")?;
    Ok(())
}
fn bound_samples(samples: &[String]) -> (Vec<String>, bool) {
    let mut truncated = samples.len() > 20;
    let lines = samples
        .iter()
        .take(20)
        .map(|line| {
            let mut end = line.len().min(2048);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            truncated |= end < line.len();
            line[..end].to_owned()
        })
        .collect();
    (lines, truncated)
}
fn check_max(max: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        (65_536..=MAX_FETCH_BYTES).contains(&max),
        "Node returned an invalid maximum bundle size"
    );
    Ok(())
}
fn pending_exact<'a>(
    listed: &'a ListLogRequestsResponse,
    id: &str,
    now: u64,
) -> anyhow::Result<&'a PendingLogRequest> {
    request_id(id).map_err(anyhow::Error::msg)?;
    let r = listed
        .requests
        .iter()
        .find(|r| hex::encode(&r.request_id) == id)
        .ok_or_else(|| anyhow::anyhow!("log request is absent; refresh the queue"))?;
    Request::from_proto(r)?;
    anyhow::ensure!(
        r.status() == LogRequestStatus::Pending && now < r.expires_unix_ms,
        "log request is no longer pending or has expired; refresh the queue"
    );
    Ok(r)
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema_version: u32,
    token: String,
    own: String,
    request_id: String,
    request_digest: String,
    expires_unix_ms: u64,
    sealed_bytes: u64,
    sealed_sha256: String,
    map_bytes: u64,
    map_sha256: String,
}
fn metadata(path: &Path) -> anyhow::Result<Metadata> {
    let m: Metadata = serde_json::from_slice(&read_private(&path.join("metadata"), MAX_META)?)?;
    anyhow::ensure!(m.schema_version == 1, "unsupported preparation metadata");
    token(&m.token).map_err(anyhow::Error::msg)?;
    request_id(&m.request_id).map_err(anyhow::Error::msg)?;
    for hash in [&m.request_digest, &m.sealed_sha256, &m.map_sha256] {
        exact_hex(hash, 64).map_err(anyhow::Error::msg)?;
    }
    anyhow::ensure!(
        m.sealed_bytes > 0 && m.sealed_bytes <= MAX_FETCH_BYTES && m.map_bytes <= MAX_MAP as u64,
        "invalid preparation size"
    );
    Ok(m)
}
fn is_missing(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
fn check_private_dir(path: &Path) -> anyhow::Result<()> {
    let m = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        m.file_type().is_dir(),
        "preparation directory is not a regular directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            m.permissions().mode() & 0o777 == 0o700,
            "preparation directory must have mode 0700"
        );
    }
    Ok(())
}
fn create_private_dir(path: &Path) -> anyhow::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    check_private_dir(path)
}
fn check_private_file(m: &std::fs::Metadata) -> anyhow::Result<()> {
    anyhow::ensure!(
        m.file_type().is_file(),
        "preparation artifact is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            m.permissions().mode() & 0o777 == 0o600 && m.nlink() == 1,
            "preparation artifact must be private and unlinked"
        );
    }
    Ok(())
}
fn read_private(path: &Path, max: usize) -> anyhow::Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    check_private_file(&before)?;
    anyhow::ensure!(
        before.len() <= max as u64,
        "preparation artifact exceeds its bound"
    );
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    // Refuse symlink swaps and do not block on a raced FIFO. No libc dependency.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(0x20000 | 0x800);
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(0x100 | 0x4);
    }
    let file = opts.open(path)?;
    let after = file.metadata()?;
    check_private_file(&after)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.uid() == after.uid(),
            "preparation artifact changed while opening"
        );
    }
    anyhow::ensure!(
        after.len() <= max as u64,
        "preparation artifact exceeds its bound"
    );
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= max && bytes.len() as u64 == after.len(),
        "preparation artifact changed or exceeded its bound"
    );
    Ok(bytes)
}
fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
struct Store {
    root: PathBuf,
    _lock: std::fs::File,
}
impl Store {
    fn open(dir: &Path, now: u64) -> anyhow::Result<Self> {
        let diagnostics = dir.join(".diagnostics");
        create_private_dir(&diagnostics)?;
        let root = diagnostics.join("prepared-log-approvals");
        create_private_dir(&root)?;
        let lock = root.join(".lock");
        match std::fs::symlink_metadata(&lock) {
            Ok(meta) => check_private_file(&meta)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(0x20000 | 0x800);
        }
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(0x100 | 0x4);
        }
        let lock = opts.open(lock)?;
        check_private_file(&lock.metadata()?)?;
        anyhow::ensure!(lock.metadata()?.len() == 0, "invalid preparation lock file");
        // The OS releases the lock on process exit, including a crash.
        lock.try_lock().map_err(|_| {
            anyhow::anyhow!(
                "another machine preparation operation is busy; refresh before retrying"
            )
        })?;
        let mut store = Self { root, _lock: lock };
        store.cleanup(now)?;
        Ok(store)
    }
    fn stage_names(&self) -> anyhow::Result<Vec<String>> {
        check_private_dir(&self.root)?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid preparation name"))?;
            if name == ".lock" {
                continue;
            }
            token(name.strip_prefix("claimed-").unwrap_or(&name)).map_err(anyhow::Error::msg)?;
            match check_private_dir(&entry.path()) {
                Ok(()) => names.push(name),
                Err(error) if is_missing(&error) => {} // A completed approval dropped its claimed directory.
                Err(error) => return Err(error),
            }
        }
        Ok(names)
    }
    fn cleanup(&mut self, now: u64) -> anyhow::Result<()> {
        for name in self.stage_names()? {
            let path = self.root.join(&name);
            let meta = match metadata(&path) {
                Ok(meta) => meta,
                Err(error) if is_missing(&error) => {
                    // A crashed writer may not have published its metadata yet.
                    // Keep fresh partial stages within the four-stage limit.
                    match std::fs::symlink_metadata(&path) {
                        Ok(info) => {
                            check_private_dir(&path)?;
                            let modified = info
                                .modified()?
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            if now >= modified.saturating_add(STAGE_TTL_MS) {
                                std::fs::remove_dir_all(&path)?;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                    continue;
                }
                Err(error) => return Err(error),
            };
            anyhow::ensure!(
                meta.token == name.strip_prefix("claimed-").unwrap_or(&name),
                "preparation token mismatch"
            );
            if now >= meta.expires_unix_ms {
                std::fs::remove_dir_all(path)?;
            }
        }
        Ok(())
    }
    fn save(
        &mut self,
        own: &str,
        request: &PendingLogRequest,
        sealed: &[u8],
        map: &BTreeMap<String, String>,
        now: u64,
    ) -> anyhow::Result<(String, u64)> {
        anyhow::ensure!(
            request.status() == LogRequestStatus::Pending && now < request.expires_unix_ms,
            "request is not pending or has expired"
        );
        anyhow::ensure!(
            !sealed.is_empty() && sealed.len() as u64 <= MAX_FETCH_BYTES,
            "invalid sealed bundle size"
        );
        let map = serde_json::to_vec(map)?;
        anyhow::ensure!(map.len() <= MAX_MAP, "private token map exceeds 16 MiB");
        self.cleanup(now)?;
        anyhow::ensure!(
            self.stage_names()?.len() < MAX_STAGES,
            "four preparations already exist; discard one or wait for expiry"
        );
        let mut random = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut random);
        let token = hex::encode(random);
        let path = self.root.join(&token);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        let mut guard = export::StagingGuard(Some(path.clone()));
        let expires = request
            .expires_unix_ms
            .min(now.saturating_add(STAGE_TTL_MS));
        let m = Metadata {
            schema_version: 1,
            token: token.clone(),
            own: own.into(),
            request_id: hex::encode(&request.request_id),
            request_digest: digest(&request.encode_to_vec()),
            expires_unix_ms: expires,
            sealed_bytes: sealed.len() as u64,
            sealed_sha256: digest(sealed),
            map_bytes: map.len() as u64,
            map_sha256: digest(&map),
        };
        write_private(&path.join("bundle"), sealed)?;
        write_private(&path.join("map"), &map)?;
        let metadata_bytes = serde_json::to_vec(&m)?;
        anyhow::ensure!(
            metadata_bytes.len() <= MAX_META,
            "preparation metadata exceeds its bound"
        );
        write_private(&path.join("metadata.partial"), &metadata_bytes)?;
        std::fs::rename(path.join("metadata.partial"), path.join("metadata"))?;
        guard.0 = None;
        Ok((token, expires))
    }
    fn claim(&mut self, token_text: &str, id: &str) -> anyhow::Result<Claim> {
        token(token_text).map_err(anyhow::Error::msg)?;
        request_id(id).map_err(anyhow::Error::msg)?;
        let from = self.root.join(token_text);
        check_private_dir(&from)?;
        let to = self.root.join(format!("claimed-{token_text}"));
        anyhow::ensure!(
            std::fs::symlink_metadata(&to).is_err(),
            "preparation already claimed"
        );
        std::fs::rename(from, &to)?;
        let claim = Claim { path: to };
        let m = metadata(&claim.path)?;
        anyhow::ensure!(
            m.token == token_text && m.request_id == id,
            "preparation belongs to another request"
        );
        Ok(claim)
    }
    fn discard(&mut self, token_text: &str, id: &str) -> anyhow::Result<()> {
        token(token_text).map_err(anyhow::Error::msg)?;
        request_id(id).map_err(anyhow::Error::msg)?;
        let path = self.root.join(token_text);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        check_private_dir(&path)?;
        let m = metadata(&path)?;
        anyhow::ensure!(
            m.request_id == id && m.token == token_text,
            "preparation belongs to another request"
        );
        std::fs::remove_dir_all(path)?;
        Ok(())
    }
    fn clear_request(&mut self, id: &str) -> anyhow::Result<()> {
        for name in self.stage_names()? {
            let path = self.root.join(name);
            match metadata(&path) {
                Ok(meta) if meta.request_id == id => std::fs::remove_dir_all(path)?,
                Ok(_) => {}
                Err(error) if is_missing(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
struct Claim {
    path: PathBuf,
}
impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
struct Frozen {
    sealed: Vec<u8>,
    map: BTreeMap<String, String>,
}
impl Claim {
    fn validate(
        &self,
        own: &str,
        request: &PendingLogRequest,
        max: u64,
        now: u64,
    ) -> anyhow::Result<Frozen> {
        check_private_dir(&self.path)?;
        let m = metadata(&self.path)?;
        anyhow::ensure!(m.own == own, "Node identity changed; prepare again");
        anyhow::ensure!(
            now < m.expires_unix_ms && now < request.expires_unix_ms,
            "preparation expired; prepare again"
        );
        anyhow::ensure!(
            request.status() == LogRequestStatus::Pending
                && m.request_id == hex::encode(&request.request_id)
                && m.request_digest == digest(&request.encode_to_vec()),
            "queued request changed; prepare again"
        );
        anyhow::ensure!(
            m.sealed_bytes <= max && max <= MAX_FETCH_BYTES,
            "current Node bundle limit no longer permits this preparation"
        );
        let sealed = read_private(&self.path.join("bundle"), m.sealed_bytes as usize)?;
        let map = read_private(&self.path.join("map"), MAX_MAP)?;
        anyhow::ensure!(
            sealed.len() as u64 == m.sealed_bytes
                && digest(&sealed) == m.sealed_sha256
                && map.len() as u64 == m.map_bytes
                && digest(&map) == m.map_sha256,
            "prepared artifacts changed; prepare again"
        );
        Ok(Frozen {
            sealed,
            map: serde_json::from_slice(&map)?,
        })
    }
}
fn prepare_produced(
    store: &mut Store,
    own: &str,
    request: &PendingLogRequest,
    max: u64,
    produced: export::Export<Capped>,
    now: u64,
) -> anyhow::Result<Preview> {
    let dto = Request::from_proto(request)?;
    let (manifest, sums) = export::bundle_texts(&produced.manifest, &produced.log)?;
    let plaintext = pack_bundle(&[
        (BUNDLE_LOG, &produced.out.data),
        ("manifest.json", manifest.as_bytes()),
        ("SHA256SUMS", sums.as_bytes()),
    ]);
    anyhow::ensure!(
        quil_crypto::seal::sealed_len(plaintext.len()) as u64 <= max,
        "sealed export exceeds current Node maximum bundle size"
    );
    let sealed = quil_crypto::seal::seal(
        &plaintext,
        &request.recipient_kem_public_key,
        &request.request_id,
    )
    .map_err(|e| anyhow::anyhow!("seal export: {e}"))?;
    let (redacted_samples, samples_truncated) = bound_samples(&produced.stats.samples);
    let source = &produced.manifest["source"];
    let preview = PreviewDetails {
        lines_read: produced.stats.lines_read,
        lines_kept: produced.stats.lines_kept,
        source_kind: source["kind"].as_str().unwrap_or("unknown").into(),
        source_count: source["files"].as_array().map_or(1, |a| a.len() as u64),
        redactions: serde_json::from_value(produced.manifest["redaction"]["counts"].clone())?,
        redacted_samples,
        samples_truncated,
        unreadable_sources: produced.stats.read_errors.len() as u64,
        note: produced.manifest["redaction"]["note"]
            .as_str()
            .unwrap_or("")
            .into(),
    };
    let (prepared_token, expires_unix_ms) =
        store.save(own, request, &sealed, &produced.token_map, now)?;
    let dto = Preview {
        schema_version: 1,
        request: dto,
        prepared_token,
        expires_unix_ms,
        sealed_bytes: sealed.len() as u64,
        preview,
    };
    if let Err(error) = json_bytes(&dto) {
        store.discard(&dto.prepared_token, &dto.request.request_id)?;
        return Err(error);
    }
    Ok(dto)
}

fn approved_result(
    request: &PendingLogRequest,
    bytes: u64,
    response: quil_types::proto::diagnostics::UploadLogBundleResponse,
    now: u64,
) -> anyhow::Result<Approved> {
    anyhow::ensure!(
        response.status() == LogRequestStatus::Ready && response.expires_unix_ms > now,
        "Node returned an unexpected approval result; refresh the queue"
    );
    Ok(Approved {
        schema_version: 1,
        request_id: hex::encode(&request.request_id),
        outcome: "approved",
        bundle_bytes: bytes,
        expires_unix_ms: response.expires_unix_ms,
    })
}

pub async fn run(
    global: GlobalArgs,
    config_dir: &Path,
    cmd: &MachineCommand,
) -> anyhow::Result<()> {
    run_with_context(config_dir, cmd, || Context::load(global)).await
}

async fn run_with_context(
    config_dir: &Path,
    cmd: &MachineCommand,
    load_context: impl FnOnce() -> anyhow::Result<Context>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        config_dir.is_absolute() && config_dir.is_dir(),
        "selected Node configuration directory does not exist"
    );
    // Consume an approval capability even when client/config parsing fails.
    let claim = if let MachineCommand::Approve { request_id, token } = cmd {
        Some(Store::open(config_dir, now_ms())?.claim(token, request_id)?)
    } else {
        None
    };
    let ctx = load_context()?;
    let (config, dir) = load_selected_config(&ctx, config_dir)?;
    // Discard never opens an RPC or requires a running Node.
    if let MachineCommand::Discard { request_id, token } = cmd {
        Store::open(&dir, now_ms())?.discard(token, request_id)?;
        return emit(&Discarded {
            schema_version: 1,
            request_id: request_id.clone(),
            outcome: "discarded",
        });
    }
    if claim.is_none() {
        drop(Store::open(&dir, now_ms())?);
    }
    let (key, own) = network_key(&ctx, &config, &dir)?;
    let own = own.to_base58();
    let addr = rpc::local_stream_addr(&config)?;
    let mut client = logs::connect_operator(&addr, &key)
        .await
        .map_err(|e| anyhow::anyhow!("connect to this Node: {}", error_chain(&*e)))?;
    match cmd {
        MachineCommand::List => emit(&list_dto(&list(&mut client).await?)?),
        MachineCommand::Prepare { request_id } => {
            let listed = list(&mut client).await?;
            check_max(listed.max_bundle_bytes)?;
            let request = pending_exact(&listed, request_id, now_ms())?;
            let selection = request_selection(request)?;
            let (readers, source, child) = export::open_selected_source(
                &config,
                &dir,
                &std::env::current_dir()?,
                selection.since,
                selection.until,
            )?;
            let requester = Request::from_proto(request)?.requester_peer_id;
            let produced = approve_export(
                readers,
                child,
                &source,
                request,
                &requester,
                listed.max_bundle_bytes as usize,
                None,
            )?;
            let preview = prepare_produced(
                &mut Store::open(&dir, now_ms())?,
                &own,
                request,
                listed.max_bundle_bytes,
                produced,
                now_ms(),
            )?;
            emit(&preview)
        }
        MachineCommand::Approve { request_id, token } => {
            let listed = list(&mut client).await?;
            check_max(listed.max_bundle_bytes)?;
            let request = pending_exact(&listed, request_id, now_ms())?;
            let frozen = claim.as_ref().expect("claimed above").validate(
                &own,
                request,
                listed.max_bundle_bytes,
                now_ms(),
            )?;
            let maps = dir.join(".diagnostics/token-maps");
            create_private_dir(&maps)?;
            prune_token_maps(&maps, std::time::SystemTime::now(), TOKEN_MAP_RETENTION);
            let map_path = maps.join(format!("log-request-{request_id}-{token}.map.txt"));
            export::write_token_map(&map_path, &format!("log request {request_id}"), &frozen.map)?;
            let upload = logs::timed(
                futures::stream::iter(upload_chunks(&request.request_id, &frozen.sealed)),
                logs::upload_timeout(frozen.sealed.len() as u64),
            );
            let response = match client.upload_log_bundle(upload).await {
                Ok(r) => r.into_inner(),
                Err(s) => {
                    let _ = std::fs::remove_file(map_path);
                    anyhow::bail!("Node did not take the prepared bundle: {:?}", s.code());
                }
            };
            emit(&approved_result(
                request,
                frozen.sealed.len() as u64,
                response,
                now_ms(),
            )?)
        }
        MachineCommand::Deny { request_id } => {
            let listed = list(&mut client).await?;
            let request = listed
                .requests
                .iter()
                .find(|r| hex::encode(&r.request_id) == *request_id)
                .ok_or_else(|| anyhow::anyhow!("request absent; refresh the queue"))?;
            let previous = Request::from_proto(request)?.status;
            anyhow::ensure!(
                previous != "unknown",
                "unknown request status is not actionable"
            );
            let response = client
                .deny_log_request(logs::timed(
                    DenyLogRequestRequest {
                        request_id: request.request_id.clone(),
                    },
                    logs::CALL_TIMEOUT,
                ))
                .await?
                .into_inner();
            anyhow::ensure!(
                response.status() == LogRequestStatus::Denied,
                "Node returned an unexpected denial result"
            );
            Store::open(&dir, now_ms())?.clear_request(request_id)?;
            emit(&Denied {
                schema_version: 1,
                request_id: request_id.clone(),
                outcome: "denied",
                previous_status: previous.into(),
            })
        }
        MachineCommand::Discard { .. } => unreachable!(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[tokio::test]
    async fn approval_attempt_consumes_preparation_on_context_or_config_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let mut r = req();
        r.received_unix_ms = now_ms();
        r.expires_unix_ms = now_ms() + 600_000;
        let id = hex::encode(&r.request_id);
        for context_failure in [false, true] {
            let token = Store::open(tmp.path(), now_ms())
                .unwrap()
                .save("own", &r, b"sealed", &BTreeMap::new(), now_ms())
                .unwrap()
                .0;
            std::fs::write(tmp.path().join("config.yml"), "[invalid: yaml").unwrap();
            let result = run_with_context(
                tmp.path(),
                &MachineCommand::Approve {
                    request_id: id.clone(),
                    token: token.clone(),
                },
                || {
                    if context_failure {
                        anyhow::bail!("synthetic client configuration failure");
                    }
                    Ok(Context {
                        global: GlobalArgs {
                            network: None,
                            signature_check: false,
                            yes: false,
                        },
                        client_config: crate::config::ClientConfig::default_for_create(),
                    })
                },
            )
            .await;
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains(if context_failure {
                    "synthetic client"
                } else {
                    "load node config"
                }),
                "{error}"
            );
            assert!(
                Store::open(tmp.path(), now_ms())
                    .unwrap()
                    .claim(&token, &id)
                    .is_err(),
                "approval errors must consume the capability"
            );
        }
    }
    #[test]
    fn interrupted_preparation_does_not_block_the_queue_and_expires() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path(), 100).unwrap();
        let path = store.root.join("ef".repeat(32));
        drop(store);
        create_private_dir(&path).unwrap();
        write_private(&path.join("bundle"), b"partial ciphertext").unwrap();
        let store = Store::open(tmp.path(), 100)
            .expect("a fresh interrupted preparation reserves capacity without blocking the queue");
        assert_eq!(store.stage_names().unwrap().len(), 1);
        drop(store);
        let times = std::fs::FileTimes::new()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(100));
        std::fs::File::open(&path)
            .unwrap()
            .set_times(times)
            .unwrap();
        assert!(Store::open(tmp.path(), 600101)
            .unwrap()
            .stage_names()
            .unwrap()
            .is_empty());
        assert!(!path.exists());
    }
    #[test]
    fn selected_config_ignores_network_default_and_never_creates_a_missing_config() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("managed/.config");
        let other = root.path().join("home/.quilibrium/configs/node-quickstart");
        for (dir, port) in [(&selected, 18340), (&other, 28340)] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(
                dir.join("config.yml"),
                format!("p2p:\n  streamListenMultiaddr: /ip4/127.0.0.1/tcp/{port}\n"),
            )
            .unwrap();
        }
        let ctx = Context {
            global: GlobalArgs {
                network: Some(other.to_string_lossy().into()),
                signature_check: false,
                yes: false,
            },
            client_config: crate::config::ClientConfig::default_for_create(),
        };
        let (config, dir) = load_selected_config(&ctx, &selected).unwrap();
        assert_eq!(dir, selected);
        assert_eq!(rpc::local_stream_addr(&config).unwrap(), "127.0.0.1:18340");
        assert!(config
            .key
            .key_store_file
            .path
            .starts_with(selected.to_str().unwrap()));
        let missing = root.path().join("missing/.config");
        assert!(load_selected_config(&ctx, &missing).is_err());
        assert!(!missing.exists());
    }
    #[test]
    fn os_lock_recovers_crashed_operations_and_excludes_concurrent_writers() {
        let tmp = tempfile::tempdir().unwrap();
        drop(Store::open(tmp.path(), 100).unwrap());
        let lock = tmp.path().join(".diagnostics/prepared-log-approvals/.lock");
        if !lock.exists() {
            write_private(&lock, b"").unwrap();
        }
        let store = Store::open(tmp.path(), 100)
            .expect("a crashed process leaves an unlocked regular lock file");
        assert!(Store::open(tmp.path(), 100).is_err());
        drop(store);
        assert!(Store::open(tmp.path(), 100).is_ok());
    }
    #[test]
    fn approval_accepts_official_node_renewed_expiry() {
        let request = req();
        let response = quil_types::proto::diagnostics::UploadLogBundleResponse {
            status: LogRequestStatus::Ready as i32,
            expires_unix_ms: request.expires_unix_ms + 1000,
        };
        let approved = approved_result(&request, 12345, response, 100).unwrap();
        assert_eq!(approved.bundle_bytes, 12345);
        assert_eq!(approved.expires_unix_ms, request.expires_unix_ms + 1000);
    }
    #[test]
    fn machine_json_contract_fixtures() {
        let r = req();
        let request_id = hex::encode(&r.request_id);
        let listed = list_dto(&ListLogRequestsResponse {
            requests: vec![r.clone()],
            max_bundle_bytes: 1 << 20,
        })
        .unwrap();
        let preview = Preview {
            schema_version: 1,
            request: Request::from_proto(&r).unwrap(),
            prepared_token: "cd".repeat(32),
            expires_unix_ms: 600100,
            sealed_bytes: 12345,
            preview: PreviewDetails {
                lines_read: 10,
                lines_kept: 8,
                source_kind: "files".into(),
                source_count: 2,
                redactions: BTreeMap::from([("ip4".into(), 3)]),
                redacted_samples: vec!["frame ip4#1234abcd".into()],
                samples_truncated: false,
                unreadable_sources: 0,
                note: "Redaction cannot guarantee the removal of every sensitive value.".into(),
            },
        };
        println!(
            "MACHINE_FIXTURE_LIST {}",
            String::from_utf8(json_bytes(&listed).unwrap()).unwrap()
        );
        println!(
            "MACHINE_FIXTURE_PREVIEW {}",
            String::from_utf8(json_bytes(&preview).unwrap()).unwrap()
        );
        println!(
            "MACHINE_FIXTURE_APPROVE {}",
            String::from_utf8(
                json_bytes(&Approved {
                    schema_version: 1,
                    request_id: request_id.clone(),
                    outcome: "approved",
                    bundle_bytes: 12345,
                    expires_unix_ms: 2_000_000
                })
                .unwrap()
            )
            .unwrap()
        );
        println!(
            "MACHINE_FIXTURE_DENY {}",
            String::from_utf8(
                json_bytes(&Denied {
                    schema_version: 1,
                    request_id: request_id.clone(),
                    outcome: "denied",
                    previous_status: "pending".into()
                })
                .unwrap()
            )
            .unwrap()
        );
        println!(
            "MACHINE_FIXTURE_DISCARD {}",
            String::from_utf8(
                json_bytes(&Discarded {
                    schema_version: 1,
                    request_id,
                    outcome: "discarded"
                })
                .unwrap()
            )
            .unwrap()
        );
    }
    fn req() -> PendingLogRequest {
        PendingLogRequest {
            request_id: vec![0xab; 16],
            requester_peer_id: quil_p2p::Keypair::generate_ed448()
                .public()
                .to_peer_id()
                .to_bytes(),
            status: LogRequestStatus::Pending as i32,
            received_unix_ms: 90,
            expires_unix_ms: 1_000_000,
            since_unix: 1_791_979_200,
            until_unix: 1_791_986_400,
            filters: vec![],
            recipient_kem_public_key: quil_crypto::Sntrup761KeyPair::generate().public,
            bundle_bytes: 0,
        }
    }
    fn staged(root: &Path, request: &PendingLogRequest) -> String {
        Store::open(root, 100)
            .unwrap()
            .save(
                "own",
                request,
                b"sealed",
                &BTreeMap::from([("ip#token".into(), "192.0.2.1".into())]),
                100,
            )
            .unwrap()
            .0
    }
    #[test]
    fn stage_freezes_and_consumes_once() {
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        let token = staged(tmp.path(), &r);
        assert_eq!(token.len(), 64);
        let mut store = Store::open(tmp.path(), 101).unwrap();
        let claim = store.claim(&token, &hex::encode(&r.request_id)).unwrap();
        let bytes = claim.validate("own", &r, 1000, 101).unwrap();
        assert_eq!(bytes.sealed, b"sealed");
        assert_eq!(bytes.map["ip#token"], "192.0.2.1");
        assert!(store.claim(&token, &hex::encode(&r.request_id)).is_err());
        drop(claim);
        assert!(!store.root.join(format!("claimed-{token}")).exists());
    }
    #[test]
    fn stage_rejects_changed_identity_request_expiry_and_limits() {
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        for kind in 0..7 {
            let token = staged(tmp.path(), &r);
            let mut store = Store::open(tmp.path(), 101).unwrap();
            let claim = store.claim(&token, &hex::encode(&r.request_id)).unwrap();
            let mut changed = r.clone();
            match kind {
                1 => changed.received_unix_ms += 1,
                2 => changed.recipient_kem_public_key[0] ^= 1,
                3 => changed.status = LogRequestStatus::Ready as i32,
                4 => changed.filters.push(vec![1; 32]),
                _ => {}
            }
            assert!(
                claim
                    .validate(
                        if kind == 0 { "other" } else { "own" },
                        &changed,
                        if kind == 5 { 1 } else { 1000 },
                        if kind == 6 { 600_100 } else { 101 }
                    )
                    .is_err(),
                "kind {kind}"
            );
        }
    }
    #[test]
    fn altered_ciphertext_and_map_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        for name in ["bundle", "map"] {
            let token = staged(tmp.path(), &r);
            std::fs::write(
                tmp.path()
                    .join(".diagnostics/prepared-log-approvals")
                    .join(&token)
                    .join(name),
                b"altered",
            )
            .unwrap();
            let claim = Store::open(tmp.path(), 101)
                .unwrap()
                .claim(&token, &hex::encode(&r.request_id))
                .unwrap();
            assert!(claim.validate("own", &r, 1000, 101).is_err());
        }
    }
    #[test]
    fn discard_is_local_idempotent_and_checks_request() {
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        let token = staged(tmp.path(), &r);
        let mut store = Store::open(tmp.path(), 101).unwrap();
        assert!(store.discard(&token, &"cd".repeat(16)).is_err());
        store.discard(&token, &hex::encode(&r.request_id)).unwrap();
        store.discard(&token, &hex::encode(&r.request_id)).unwrap();
        assert_eq!(r.status(), LogRequestStatus::Pending);
        assert!(store
            .discard("../escape", &hex::encode(&r.request_id))
            .is_err());
    }
    #[test]
    fn live_stages_are_bounded_and_expired_claims_cleaned() {
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        for _ in 0..4 {
            staged(tmp.path(), &r);
        }
        assert!(Store::open(tmp.path(), 100)
            .unwrap()
            .save("own", &r, b"sealed", &BTreeMap::new(), 100)
            .is_err());
        let mut store = Store::open(tmp.path(), 600_101).unwrap();
        assert_eq!(store.stage_names().unwrap().len(), 0);
        let (token, _) = store
            .save("own", &r, b"sealed", &BTreeMap::new(), 600_101)
            .unwrap();
        let claim = store.claim(&token, &hex::encode(&r.request_id)).unwrap();
        std::mem::forget(claim);
        drop(store);
        assert_eq!(
            Store::open(tmp.path(), 1_000_001)
                .unwrap()
                .stage_names()
                .unwrap()
                .len(),
            0
        );
    }
    #[test]
    fn private_map_size_and_stdout_are_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        let map = BTreeMap::from([("token".into(), "x".repeat(MAX_MAP))]);
        assert!(Store::open(tmp.path(), 100)
            .unwrap()
            .save("own", &r, b"sealed", &map, 100)
            .is_err());
        assert!(json_bytes(&serde_json::json!({"large":"x".repeat(MAX_JSON)})).is_err());
        let sample = "\u{754c}".repeat(2000);
        let (bounded, truncated) = bound_samples(&[sample]);
        assert!(truncated);
        assert!(bounded[0].len() <= 2048);
        assert!(std::str::from_utf8(bounded[0].as_bytes()).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_hardlinks_and_permissions_fail_closed() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        let r = req();
        for kind in 0..3 {
            let token = staged(tmp.path(), &r);
            let path = tmp
                .path()
                .join(".diagnostics/prepared-log-approvals")
                .join(&token)
                .join("bundle");
            let outside = tmp.path().join(format!("outside-{kind}"));
            std::fs::write(&outside, b"sealed").unwrap();
            match kind {
                0 => {
                    std::fs::remove_file(&path).unwrap();
                    symlink(&outside, &path).unwrap();
                }
                1 => {
                    std::fs::hard_link(&path, &outside.with_extension("link")).unwrap();
                }
                _ => {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap()
                }
            }
            let claim = Store::open(tmp.path(), 101)
                .unwrap()
                .claim(&token, &hex::encode(&r.request_id))
                .unwrap();
            assert!(claim.validate("own", &r, 1000, 101).is_err());
        }
        let bad = tempfile::tempdir().unwrap();
        symlink(tmp.path(), bad.path().join(".diagnostics")).unwrap();
        assert!(Store::open(bad.path(), 100).is_err());
    }
    #[test]
    fn prepared_export_uses_real_redaction_seal_and_checksums_without_upload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut r = req();
        let kem = quil_crypto::Sntrup761KeyPair::generate();
        r.recipient_kem_public_key = kem.public;
        let src = tempfile::tempdir().unwrap();
        std::fs::write(
            src.path().join("master.log"),
            "2026-10-14T12:30:00Z\tinfo\tnode.rs:7\tframe\t{\"peer\":\"192.0.2.7:8336\"}\n",
        )
        .unwrap();
        let (readers, names) =
            export::file_readers(src.path(), r.since_unix as i64, r.until_unix as i64).unwrap();
        let produced = approve_export(
            readers,
            None,
            &export::SourceInfo::Files(names),
            &r,
            "requester",
            1 << 20,
            Some([7; 32]),
        )
        .unwrap();
        let mut store = Store::open(tmp.path(), 100).unwrap();
        let preview = prepare_produced(&mut store, "own", &r, 1 << 20, produced, 100).unwrap();
        let value = serde_json::to_value(&preview).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["request"]["status"], "pending");
        let text = String::from_utf8(json_bytes(&preview).unwrap()).unwrap();
        assert!(!text.contains("192.0.2.7"));
        assert!(!text.contains("token_map"));
        assert!(!text.contains("master.log"));
        let claim = store
            .claim(&preview.prepared_token, &hex::encode(&r.request_id))
            .unwrap();
        let frozen = claim.validate("own", &r, 1 << 20, 101).unwrap();
        let plain = quil_crypto::seal::open(&frozen.sealed, &kem.secret, &r.request_id).unwrap();
        let files = unpack_bundle(&plain).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(preview.sealed_bytes, frozen.sealed.len() as u64);
        assert!(!frozen.map.is_empty());
        assert_eq!(r.status(), LogRequestStatus::Pending);
    }
}
