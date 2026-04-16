use std::fs;
use std::io::{self, Write};
use std::net::{IpAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use sendrs_discovery::{
    broadcast_share_offer, discover_snapshot, DiscoveryBeacon, DiscoverySnapshot,
    SharedOfferAnnouncement, DEFAULT_DISCOVERY_PORT,
};
use sendrs_security::{load_or_create_identity, load_trust_store, pair_peer};
use sendrs_transfer::{
    build_manifest, execute_local_transfer, save_manifest, total_size, DEFAULT_CHUNK_SIZE,
    DEFAULT_MAX_RETRIES,
};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(author, version, about = "send-rs CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Discover {
        #[arg(long, default_value_t = 2)]
        timeout_secs: u64,
    },
    Send {
        path: PathBuf,
        #[arg(long, default_value_t = false)]
        public: bool,
    },
    Receive {
        code: String,
        #[arg(long)]
        target: Option<PathBuf>,
    },
    History {
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    Clean {
        #[arg(long, default_value_t = false)]
        all: bool,
        #[arg(long, default_value_t = false)]
        sessions: bool,
        #[arg(long, default_value_t = false)]
        offers: bool,
        #[arg(long, default_value_t = false)]
        manifests: bool,
        #[arg(long, default_value_t = false)]
        history: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SendOffer {
    code: String,
    sender_id: String,
    sender_name: String,
    source_path: String,
    manifest_path: String,
    public_enabled: bool,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReceiverClaim {
    claim_id: String,
    receiver_id: String,
    receiver_name: String,
    target_dir: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionState {
    #[serde(default)]
    claim_id: Option<String>,
    state: String,
    message: String,
    bytes_done: u64,
    bytes_total: u64,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransferHistoryEntry {
    id: String,
    code: String,
    role: String,
    status: String,
    source: Option<String>,
    target: Option<String>,
    peer_id: Option<String>,
    peer_name: Option<String>,
    bytes_done: u64,
    bytes_total: u64,
    started_at: String,
    finished_at: String,
    error: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Discover { timeout_secs } => cmd_discover(timeout_secs),
        Command::Send { path, public } => cmd_send(&path, public),
        Command::Receive { code, target } => cmd_receive(&code, target.as_deref()),
        Command::History { limit } => cmd_history(limit),
        Command::Clean {
            all,
            sessions,
            offers,
            manifests,
            history,
        } => cmd_clean(all, sessions, offers, manifests, history),
    }
}

fn cmd_discover(timeout_secs: u64) -> Result<()> {
    let identity = load_or_create_identity(None)?;
    let beacon = DiscoveryBeacon::from_identity(&identity.identity, 38080, false);
    let snapshot: DiscoverySnapshot =
        discover_snapshot(&beacon, Duration::from_secs(timeout_secs))?;

    let mut offers = snapshot.offers;
    offers.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    if offers.is_empty() {
        println!("No active shares found on LAN.");
        return Ok(());
    }
    for offer in offers {
        let kind = if offer.is_dir { "Dir" } else { "File" };
        println!(
            "{}  {kind}:{}  Code:{}",
            offer.owner_name, offer.source_name, offer.code
        );
    }
    Ok(())
}

fn cmd_send(path: &Path, public: bool) -> Result<()> {
    if !path.exists() {
        return Err(anyhow::anyhow!("path does not exist: {}", path.display()));
    }

    let identity = load_or_create_identity(None)?;
    let code = generate_code();
    let mut manifest = build_manifest(path, DEFAULT_CHUNK_SIZE)?;
    let bytes_total = total_size(&manifest);

    fs::create_dir_all(manifests_root()).context("create manifests root")?;
    fs::create_dir_all(offers_root()).context("create offers root")?;
    fs::create_dir_all(session_root(&code)).context("create session root")?;

    let manifest_path = manifest_path_by_code(&code);
    save_manifest(&manifest_path, &manifest)?;

    let sender_label = local_device_label();
    let offer = SendOffer {
        code: code.clone(),
        sender_id: identity.identity.device_id.clone(),
        sender_name: sender_label.clone(),
        source_path: path.canonicalize()?.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        public_enabled: public,
        created_at: Utc::now().to_rfc3339(),
    };
    write_json(offer_path(&code), &offer)?;

    write_state(
        &code,
        None,
        "waiting_receiver",
        "waiting receiver to enter code",
        0,
        bytes_total,
    )?;

    let announce = SharedOfferAnnouncement {
        code: code.clone(),
        owner_peer_id: identity.identity.device_id.clone(),
        owner_name: sender_label,
        source_name: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("shared")
            .to_string(),
        is_dir: path.is_dir(),
        bytes_total,
        public_enabled: public,
        created_at: Utc::now().to_rfc3339(),
    };

    let announce_clone = announce.clone();
    let _broadcast_thread = thread::spawn(move || loop {
        let _ = broadcast_share_offer(&announce_clone, DEFAULT_DISCOVERY_PORT);
        thread::sleep(Duration::from_secs(1));
    });

    println!("Send code: {}", code);
    println!("Waiting for receiver... (command will keep running, Ctrl+C to stop)");

    loop {
        clear_runtime_session_files(&code)?;
        let started_at = Utc::now().to_rfc3339();

        let claim = wait_for_receiver_claim(&code)?;
        let _ = remove_path_if_exists(receiver_claim_path(&code));

        println!(
            "Receiver connected: {} ({})",
            claim.receiver_name, claim.receiver_id
        );

        let trusted = is_trusted(&claim.receiver_id)?;
        if !trusted {
            println!("First-time connection detected.");
            println!(
                "Type 'yes' to approve this receiver: {}",
                claim.receiver_name
            );
            if !prompt_yes()? {
                let err = anyhow::anyhow!("sender rejected authentication");
                write_state(
                    &code,
                    Some(&claim.claim_id),
                    "failed",
                    &err.to_string(),
                    0,
                    bytes_total,
                )?;
                append_history(TransferHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    code: code.clone(),
                    role: "send".to_string(),
                    status: "failed".to_string(),
                    source: Some(offer.source_path.clone()),
                    target: Some(claim.target_dir.clone()),
                    peer_id: Some(claim.receiver_id.clone()),
                    peer_name: Some(claim.receiver_name.clone()),
                    bytes_done: 0,
                    bytes_total,
                    started_at,
                    finished_at: Utc::now().to_rfc3339(),
                    error: Some(err.to_string()),
                })?;
                println!("Receiver was rejected. Still sharing, waiting next receiver...");
                continue;
            }
            fs::write(sender_yes_path(&code), b"yes").context("write sender yes")?;
            println!("Waiting receiver confirmation...");
            wait_for_file(receiver_yes_path(&code))?;
            pair_peer(&claim.receiver_id, "trusted-by-yes")?;
        }

        let mut last_print = 0_u64;
        let run_result = execute_local_transfer(
            PathBuf::from(&offer.source_path),
            PathBuf::from(&claim.target_dir),
            &mut manifest,
            Some(&manifest_path),
            DEFAULT_MAX_RETRIES,
            |ev| {
                let _ = write_state(
                    &code,
                    Some(&claim.claim_id),
                    "transferring",
                    &format!("transferring {}", ev.current_file),
                    ev.bytes_done,
                    ev.bytes_total,
                );
                if ev.bytes_done.saturating_sub(last_print) >= 2 * 1024 * 1024
                    || ev.bytes_done == ev.bytes_total
                {
                    println!("progress: {}/{} bytes", ev.bytes_done, ev.bytes_total);
                    last_print = ev.bytes_done;
                }
            },
        );

        match run_result {
            Ok(report) => {
                save_manifest(&manifest_path, &manifest)?;
                write_state(
                    &code,
                    Some(&claim.claim_id),
                    "completed",
                    "transfer completed",
                    report.bytes_done,
                    report.bytes_total,
                )?;
                append_history(TransferHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    code: code.clone(),
                    role: "send".to_string(),
                    status: "completed".to_string(),
                    source: Some(offer.source_path.clone()),
                    target: Some(claim.target_dir.clone()),
                    peer_id: Some(claim.receiver_id.clone()),
                    peer_name: Some(claim.receiver_name.clone()),
                    bytes_done: report.bytes_done,
                    bytes_total: report.bytes_total,
                    started_at,
                    finished_at: Utc::now().to_rfc3339(),
                    error: None,
                })?;
                println!(
                    "Transfer completed. files={}/{} bytes={}/{}",
                    report.files_completed,
                    report.files_total,
                    report.bytes_done,
                    report.bytes_total
                );
                println!("Still sharing. Waiting next receiver...");
            }
            Err(err) => {
                write_state(
                    &code,
                    Some(&claim.claim_id),
                    "failed",
                    &err.to_string(),
                    last_print,
                    bytes_total,
                )?;
                append_history(TransferHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    code: code.clone(),
                    role: "send".to_string(),
                    status: "failed".to_string(),
                    source: Some(offer.source_path.clone()),
                    target: Some(claim.target_dir.clone()),
                    peer_id: Some(claim.receiver_id.clone()),
                    peer_name: Some(claim.receiver_name.clone()),
                    bytes_done: last_print,
                    bytes_total,
                    started_at,
                    finished_at: Utc::now().to_rfc3339(),
                    error: Some(err.to_string()),
                })?;
                println!("Transfer failed: {err}");
                println!("Still sharing. Waiting next receiver...");
            }
        }
    }
}

fn cmd_receive(code: &str, target: Option<&Path>) -> Result<()> {
    let code = normalize_code(code);
    let offer: SendOffer =
        read_json(offer_path(&code)).with_context(|| format!("invalid or expired code: {code}"))?;
    let identity = load_or_create_identity(None)?;
    let started_at = Utc::now().to_rfc3339();

    let target_dir = target
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir().context("read current directory")?);
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("create target dir {}", target_dir.display()))?;

    fs::create_dir_all(session_root(&code)).context("create session root")?;
    let claim = ReceiverClaim {
        claim_id: uuid::Uuid::new_v4().to_string(),
        receiver_id: identity.identity.device_id.clone(),
        receiver_name: identity.identity.display_name.clone(),
        target_dir: target_dir.canonicalize()?.to_string_lossy().to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    write_json(receiver_claim_path(&code), &claim)?;

    let trusted = is_trusted(&offer.sender_id)?;
    if !trusted {
        println!("First-time connection with sender: {}", offer.sender_name);
        println!("Type 'yes' to approve sender.");
        if !prompt_yes()? {
            let err = anyhow::anyhow!("receiver rejected authentication");
            append_history(TransferHistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                code: code.clone(),
                role: "receive".to_string(),
                status: "failed".to_string(),
                source: Some(offer.source_path.clone()),
                target: Some(claim.target_dir.clone()),
                peer_id: Some(offer.sender_id.clone()),
                peer_name: Some(offer.sender_name.clone()),
                bytes_done: 0,
                bytes_total: 0,
                started_at,
                finished_at: Utc::now().to_rfc3339(),
                error: Some(err.to_string()),
            })?;
            return Err(err);
        }
        fs::write(receiver_yes_path(&code), b"yes").context("write receiver yes")?;
        println!("Waiting sender confirmation...");
        wait_for_file(sender_yes_path(&code))?;
        pair_peer(&offer.sender_id, "trusted-by-yes")?;
    }

    println!("Waiting transfer to complete...");
    let state = wait_for_completion(&code, &claim.claim_id)?;
    append_history(TransferHistoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        code: code.clone(),
        role: "receive".to_string(),
        status: state.state.clone(),
        source: Some(offer.source_path.clone()),
        target: Some(claim.target_dir.clone()),
        peer_id: Some(offer.sender_id.clone()),
        peer_name: Some(offer.sender_name.clone()),
        bytes_done: state.bytes_done,
        bytes_total: state.bytes_total,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        error: if state.state == "completed" {
            None
        } else {
            Some(state.message)
        },
    })?;
    Ok(())
}

fn cmd_history(limit: usize) -> Result<()> {
    let mut entries = load_history()?;
    entries.reverse();
    let out = entries.into_iter().take(limit).collect::<Vec<_>>();
    if out.is_empty() {
        println!("No transfer history.");
        return Ok(());
    }
    for item in out {
        let source = item.source.unwrap_or_else(|| "-".to_string());
        let target = item.target.unwrap_or_else(|| "-".to_string());
        let peer = item
            .peer_name
            .or(item.peer_id)
            .unwrap_or_else(|| "Unknown".to_string());
        let file_name = Path::new(&source)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&source)
            .to_string();
        let size = human_bytes(item.bytes_total);
        let status = item.status;
        let direction = if item.role.eq_ignore_ascii_case("receive") {
            "RECV"
        } else {
            "SEND"
        };
        println!(
            "{direction} From:{peer} File:{file_name} Size:{size} Saved:{target} Status:{status}"
        );
    }
    Ok(())
}

fn cmd_clean(
    all: bool,
    sessions: bool,
    offers: bool,
    manifests: bool,
    history: bool,
) -> Result<()> {
    let mut cleaned = Vec::new();
    let no_flags = !all && !sessions && !offers && !manifests && !history;

    let should_clean_sessions = all || sessions || no_flags;
    let should_clean_offers = all || offers || no_flags;
    let should_clean_manifests = all || manifests;
    let should_clean_history = all || history;

    if should_clean_sessions {
        remove_path_if_exists(sessions_root())?;
        cleaned.push("sessions");
    }
    if should_clean_offers {
        remove_path_if_exists(offers_root())?;
        cleaned.push("offers");
    }
    if should_clean_manifests {
        remove_path_if_exists(manifests_root())?;
        cleaned.push("manifests");
    }
    if should_clean_history {
        remove_path_if_exists(history_path())?;
        cleaned.push("history");
    }

    println!("cleaned: {}", cleaned.join(", "));
    Ok(())
}

fn wait_for_receiver_claim(code: &str) -> Result<ReceiverClaim> {
    let path = receiver_claim_path(code);
    loop {
        if path.exists() {
            match read_json(&path) {
                Ok(claim) => return Ok(claim),
                Err(_) => {
                    thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn clear_runtime_session_files(code: &str) -> Result<()> {
    let _ = remove_path_if_exists(receiver_claim_path(code));
    let _ = remove_path_if_exists(sender_yes_path(code));
    let _ = remove_path_if_exists(receiver_yes_path(code));
    Ok(())
}

fn wait_for_completion(code: &str, claim_id: &str) -> Result<SessionState> {
    loop {
        let state = read_state(code)?;
        if state.claim_id.as_deref() != Some(claim_id) {
            thread::sleep(Duration::from_millis(300));
            continue;
        }
        match state.state.as_str() {
            "completed" => {
                println!("Done. bytes={}/{}", state.bytes_done, state.bytes_total);
                return Ok(state);
            }
            "failed" => {
                return Err(anyhow::anyhow!("transfer failed: {}", state.message));
            }
            _ => thread::sleep(Duration::from_secs(1)),
        }
    }
}

fn wait_for_file(path: PathBuf) -> Result<()> {
    loop {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn is_trusted(peer_id: &str) -> Result<bool> {
    let store = load_trust_store()?;
    Ok(store.peers.contains_key(peer_id))
}

fn prompt_yes() -> Result<bool> {
    print!("> ");
    io::stdout().flush().context("flush stdout")?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).context("read input")?;
    Ok(input.trim().eq_ignore_ascii_case("yes"))
}

fn write_state(
    code: &str,
    claim_id: Option<&str>,
    state: &str,
    message: &str,
    bytes_done: u64,
    bytes_total: u64,
) -> Result<()> {
    let body = SessionState {
        claim_id: claim_id.map(std::string::ToString::to_string),
        state: state.to_string(),
        message: message.to_string(),
        bytes_done,
        bytes_total,
        updated_at: Utc::now().to_rfc3339(),
    };
    write_json(state_path(code), &body)
}

fn read_state(code: &str) -> Result<SessionState> {
    read_json(state_path(code))
}

fn append_history(entry: TransferHistoryEntry) -> Result<()> {
    let mut entries = load_history()?;
    entries.push(entry);
    write_json(history_path(), &entries)
}

fn load_history() -> Result<Vec<TransferHistoryEntry>> {
    let path = history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(path)
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent).context("create parent dir")?;
    }
    let body = serde_json::to_string_pretty(value).context("serialize json")?;
    fs::write(path, body).context("write json")?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T> {
    let body = fs::read_to_string(path).context("read json")?;
    serde_json::from_str(&body).context("parse json")
}

fn remove_path_if_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("remove dir {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("remove file {}", path.display()))?;
    }
    Ok(())
}

fn data_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".sendrs")
}

fn manifests_root() -> PathBuf {
    data_root().join("manifests")
}

fn offers_root() -> PathBuf {
    data_root().join("offers")
}

fn sessions_root() -> PathBuf {
    data_root().join("sessions")
}

fn history_path() -> PathBuf {
    data_root().join("history.json")
}

fn session_root(code: &str) -> PathBuf {
    sessions_root().join(normalize_code(code))
}

fn offer_path(code: &str) -> PathBuf {
    offers_root().join(format!("{}.json", normalize_code(code)))
}

fn manifest_path_by_code(code: &str) -> PathBuf {
    manifests_root().join(format!("{}.json", normalize_code(code)))
}

fn receiver_claim_path(code: &str) -> PathBuf {
    session_root(code).join("receiver.json")
}

fn sender_yes_path(code: &str) -> PathBuf {
    session_root(code).join("sender_yes")
}

fn receiver_yes_path(code: &str) -> PathBuf {
    session_root(code).join("receiver_yes")
}

fn state_path(code: &str) -> PathBuf {
    session_root(code).join("state.json")
}

fn generate_code() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string().to_uppercase();
    format!("{}-{}", &id[0..4], &id[4..8])
}

fn normalize_code(raw: &str) -> String {
    raw.trim().to_uppercase()
}

fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let v = n as f64;
    if v >= GB {
        format!("{:.2}GB", v / GB)
    } else if v >= MB {
        format!("{:.2}MB", v / MB)
    } else if v >= KB {
        format!("{:.2}KB", v / KB)
    } else {
        format!("{n}B")
    }
}

fn local_device_label() -> String {
    let hostname = preferred_hostname();
    match local_ipv4() {
        Some(ip) => format!("{hostname}({ip})"),
        None => hostname,
    }
}

fn local_ipv4() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    let ip = addr.ip();
    if is_preferred_lan_ip(&ip) {
        return Some(ip);
    }
    private_ipv4_from_ifconfig()
}

fn preferred_hostname() -> String {
    if cfg!(target_os = "macos") {
        if let Some(name) = command_output("scutil", &["--get", "ComputerName"]) {
            return name;
        }
    }
    if let Some(name) = std::env::var("COMPUTERNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return name;
    }
    if let Some(name) = std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return name;
    }
    command_output("hostname", &[]).unwrap_or_else(|| "Computer".to_string())
}

fn command_output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = ProcCommand::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn private_ipv4_from_ifconfig() -> Option<IpAddr> {
    let text = command_output("ifconfig", &[])?;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("inet ") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _inet = parts.next()?;
        let raw = parts.next()?;
        if let Ok(ip) = raw.parse::<IpAddr>() {
            if is_preferred_lan_ip(&ip) {
                return Some(ip);
            }
        }
    }
    None
}

fn is_preferred_lan_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            if a == 10 {
                return true;
            }
            if a == 172 && (16..=31).contains(&b) {
                return true;
            }
            if a == 192 && b == 168 {
                return true;
            }
            false
        }
        IpAddr::V6(_) => false,
    }
}
