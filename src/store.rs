//! Local state. `~/.claudeconnect/` holds identity, peer permissions and room history;
//! `<project>/.claudeconnect/` holds the per-project journal and received files.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use iroh::{EndpointAddr, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::Member;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Random hex from a fresh secret key (a CSPRNG we already depend on).
pub fn random_hex(bytes: usize) -> String {
    SecretKey::generate().to_bytes()[..bytes].iter().map(|b| format!("{b:02x}")).collect()
}

pub fn home() -> PathBuf {
    let base = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_else(|| ".".into());
    PathBuf::from(base).join(".claudeconnect")
}

pub fn project_dir() -> PathBuf {
    std::env::var_os("CLAUDE_PROJECT_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()))
}

fn read_json<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, serde_json::to_vec_pretty(v)?).with_context(|| format!("writing {}", path.display()))
}

/// Stable 64-bit FNV-1a, used to tell projects apart in file names.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn slug(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' }).collect()
}

// ---------- identity ----------

pub struct Identity {
    pub me: Member,
    pub key: SecretKey,
    /// Held open for the life of the process so a second session can't take the same identity.
    _lock: File,
}

pub fn load_identity() -> anyhow::Result<Identity> {
    let home = home();
    fs::create_dir_all(home.join("sessions"))?;

    let pc_id_path = home.join("machine.id");
    let pc_id = match fs::read_to_string(&pc_id_path) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            let id = random_hex(4);
            fs::write(&pc_id_path, &id)?;
            id
        }
    };
    let pc_name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "pc".into());
    let project = project_dir();
    let folder = project.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "session".into());
    let base = std::env::var("CLAUDECONNECT_NAME").unwrap_or_else(|_| format!("{}-{}", pc_name.to_lowercase(), folder));
    let project_hash = format!("{:08x}", fnv(&project.to_string_lossy()) as u32);

    // Same name + project = same identity across restarts, so rooms can be rejoined.
    // A second live session with that name gets "-2", "-3", ...
    for n in 1..100 {
        let name = if n == 1 { base.clone() } else { format!("{base}-{n}") };
        let stem = home.join("sessions").join(format!("{}-{project_hash}", slug(&name)));
        let lock = OpenOptions::new().create(true).write(true).truncate(false).open(stem.with_extension("lock"))?;
        if lock.try_lock().is_err() {
            continue;
        }
        let key_path = stem.with_extension("key");
        let key = match fs::read(&key_path).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) {
            Some(bytes) => SecretKey::from_bytes(&bytes),
            None => {
                let k = SecretKey::generate();
                fs::write(&key_path, k.to_bytes())?;
                k
            }
        };
        let me = Member { session_id: key.public().to_string(), name, pc_id: pc_id.clone(), pc_name: pc_name.clone() };
        return Ok(Identity { me, key, _lock: lock });
    }
    anyhow::bail!("too many sessions named {base}")
}

// ---------- peer permissions ----------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Trust {
    Trusted,
    #[default]
    Untrusted,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, PartialOrd, Default)]
#[serde(rename_all = "lowercase")]
pub enum Delegate {
    Off,
    #[default]
    Manual,
    Auto,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Perms {
    /// Last known name, for humans reading peers.json.
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub receive: bool,
    #[serde(default)]
    pub trust: Trust,
    #[serde(default)]
    pub delegate: Delegate,
    /// Accepted `custom` topics; empty accepts all.
    #[serde(default)]
    pub topics: Vec<String>,
}

impl Default for Perms {
    fn default() -> Self {
        Self { name: String::new(), receive: true, trust: Trust::Untrusted, delegate: Delegate::Manual, topics: vec![] }
    }
}

impl Perms {
    /// Untrusted peers never get automatic delegation.
    pub fn effective_delegate(&self) -> Delegate {
        if self.trust == Trust::Untrusted && self.delegate == Delegate::Auto { Delegate::Manual } else { self.delegate }
    }

    pub fn accepts_topic(&self, topic: Option<&str>) -> bool {
        self.topics.is_empty() || topic.is_some_and(|t| self.topics.iter().any(|x| x == t))
    }

    /// True when `new` lets the peer do more than `self` does. Raising needs a human.
    pub fn is_raised_by(&self, new: &Perms) -> bool {
        let wider_topics = !self.topics.is_empty() && (new.topics.is_empty() || new.topics.iter().any(|t| !self.topics.contains(t)));
        (new.receive && !self.receive)
            || (new.trust == Trust::Trusted && self.trust == Trust::Untrusted)
            || new.delegate > self.delegate
            || wider_topics
    }

    /// Apply `key=value` settings, e.g. `trust=trusted`, `delegate=auto`, `topics=a,b`.
    pub fn apply(&mut self, settings: &[(String, String)]) -> anyhow::Result<()> {
        for (k, v) in settings {
            let quoted = Value::String(v.clone());
            match k.as_str() {
                "receive" => self.receive = matches!(v.as_str(), "on" | "true" | "yes"),
                "trust" => self.trust = serde_json::from_value(quoted).context("trust is trusted|untrusted")?,
                "delegate" => self.delegate = serde_json::from_value(quoted).context("delegate is off|manual|auto")?,
                "topics" => self.topics = v.split(',').map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect(),
                _ => anyhow::bail!("unknown setting {k} (receive, trust, delegate, topics)"),
            }
        }
        Ok(())
    }
}

pub fn load_peers() -> BTreeMap<String, Perms> {
    read_json(&home().join("peers.json"))
}

pub fn save_peers(peers: &BTreeMap<String, Perms>) -> anyhow::Result<()> {
    write_json(&home().join("peers.json"), peers)
}

/// Find a peer by session id prefix or exact name.
pub fn find_peer(peers: &BTreeMap<String, Perms>, query: &str) -> anyhow::Result<String> {
    let hits: Vec<_> = peers.iter().filter(|(id, p)| id.starts_with(query) || p.name == query).map(|(id, _)| id.clone()).collect();
    match hits.len() {
        1 => Ok(hits[0].clone()),
        0 => anyhow::bail!("no peer matches {query}"),
        _ => anyhow::bail!("{query} matches several peers, use more of the session id"),
    }
}

// ---------- room history ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomRecord {
    pub name: String,
    pub secret: String,
    pub members: Vec<(Member, EndpointAddr)>,
    pub last_seen: u64,
    /// The session that was in this room, so history is per session.
    pub session: String,
}

pub fn load_rooms() -> BTreeMap<String, RoomRecord> {
    read_json(&home().join("rooms.json"))
}

pub fn save_room(id: &str, rec: RoomRecord) -> anyhow::Result<()> {
    let mut rooms = load_rooms();
    rooms.insert(id.to_string(), rec);
    write_json(&home().join("rooms.json"), &rooms)
}

// ---------- project journal ----------

#[derive(Serialize, Deserialize)]
struct ProjectConfig {
    journal: bool,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self { journal: true }
    }
}

pub fn project_data() -> PathBuf {
    project_dir().join(".claudeconnect")
}

pub fn journal_enabled() -> bool {
    read_json::<ProjectConfig>(&project_data().join("config.json")).journal
}

pub fn set_journal(on: bool) -> anyhow::Result<()> {
    write_json(&project_data().join("config.json"), &ProjectConfig { journal: on })
}

pub fn journal(entry: Value) {
    if !journal_enabled() {
        return;
    }
    let dir = project_data();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    exclude_from_git();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("journal.jsonl")) {
        let _ = writeln!(f, "{entry}");
    }
}

/// Save a received file under `.claudeconnect/files/` and return its path.
pub fn save_file(msg_id: &str, name: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    let dir = project_data().join("files");
    fs::create_dir_all(&dir)?;
    exclude_from_git();
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
    let path = dir.join(format!("{msg_id}-{safe}"));
    fs::write(&path, bytes)?;
    Ok(path)
}

/// Keep `.claudeconnect/` out of git without touching the project's own .gitignore.
fn exclude_from_git() {
    let exclude = project_dir().join(".git").join("info").join("exclude");
    if !exclude.parent().is_some_and(Path::exists) {
        return;
    }
    let current = fs::read_to_string(&exclude).unwrap_or_default();
    if !current.lines().any(|l| l.trim() == ".claudeconnect/") {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&exclude) {
            let _ = writeln!(f, "{}.claudeconnect/", if current.is_empty() || current.ends_with('\n') { "" } else { "\n" });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(k: &str, v: &str) -> (String, String) {
        (k.into(), v.into())
    }

    #[test]
    fn untrusted_peer_never_gets_auto_delegation() {
        let mut p = Perms::default();
        p.apply(&[kv("delegate", "auto")]).unwrap();
        assert_eq!(p.effective_delegate(), Delegate::Manual);
        p.apply(&[kv("trust", "trusted")]).unwrap();
        assert_eq!(p.effective_delegate(), Delegate::Auto);
    }

    #[test]
    fn raising_is_detected() {
        let base = Perms::default();
        let mut more = base.clone();
        more.apply(&[kv("trust", "trusted")]).unwrap();
        assert!(base.is_raised_by(&more));
        let mut less = base.clone();
        less.apply(&[kv("delegate", "off"), kv("receive", "off")]).unwrap();
        assert!(!base.is_raised_by(&less));
        assert!(less.is_raised_by(&base));
        let mut narrow = base.clone();
        narrow.apply(&[kv("topics", "a")]).unwrap();
        assert!(!base.is_raised_by(&narrow));
        assert!(narrow.is_raised_by(&base));
    }

    #[test]
    fn topics_filter() {
        let mut p = Perms::default();
        assert!(p.accepts_topic(None));
        p.apply(&[kv("topics", "claude-mem.observation")]).unwrap();
        assert!(p.accepts_topic(Some("claude-mem.observation")));
        assert!(!p.accepts_topic(Some("other")));
        assert!(p.apply(&[kv("bogus", "1")]).is_err());
    }
}
