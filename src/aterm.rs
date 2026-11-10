// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! aterm harness contract 3: the session's scoped claim and its screen/policy witnesses.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::env::Env;

/// The contract this harness requires before it claims a session.
pub const CONTRACT: u32 = 3;
/// The duties this build takes from aterm.
pub const SCOPE: &str = "relaunch,upgrade";
/// The claim holder prefix, followed by this process's pid.
pub const HOLDER_PREFIX: &str = "kimi-alab@";
/// aterm's session-id variable.
pub const SID_ENV: &str = "ATERM_PARENT_SESSION_ID";
/// The variable that leaves the harness out of a launch.
pub const NO_HARNESS_ENV: &str = "ATERM_NO_HARNESS";
/// One socket operation's longest wait.
pub const TIMEOUT: Duration = Duration::from_secs(5);

const MAX_TOKEN_BYTES: u64 = 256;
const MAX_GRAPH_BYTES: u64 = 16 * 1024;
const MAX_LINE_BYTES: u64 = 1 << 20;

/// Whether this process runs in an aterm tab.
#[must_use]
pub fn under_aterm(env: &Env) -> bool {
    env.get("TERM_PROGRAM").is_some_and(|v| v == "aterm")
        || env.get("ATERM_CHILD").is_some_and(|v| v == "1")
}

/// Whether the person left the harness out.
#[must_use]
pub fn left_out(env: &Env) -> bool {
    env.get(NO_HARNESS_ENV).is_some_and(|v| v == "1")
}

/// aterm's rendezvous directory.
#[must_use]
pub fn control_dir(env: &Env) -> Option<PathBuf> {
    if let Some(xdg) = env.get_nonempty("XDG_RUNTIME_DIR") {
        return Some(PathBuf::from(xdg).join("aterm"));
    }
    Some(
        env.home()?
            .join("Library")
            .join("Application Support")
            .join("aterm"),
    )
}

/// Whether `sid` is a session id and never a path.
#[must_use]
pub fn valid_sid(sid: &str) -> bool {
    sid.strip_prefix("s-")
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The socket named by a graph entry's `sock ` line.
#[must_use]
pub fn graph_sock(body: &str) -> Option<PathBuf> {
    body.lines()
        .find_map(|line| line.strip_prefix("sock "))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Where a session's control socket is found, including after an aterm restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locator {
    /// The rendezvous directory.
    pub dir: PathBuf,
    /// The session id.
    pub sid: String,
}

impl Locator {
    /// Read the session locator from `env`.
    ///
    /// # Errors
    /// The session id is absent or invalid, or no rendezvous directory can be named.
    pub fn from_env(env: &Env) -> Result<Self, String> {
        let sid = env
            .get(SID_ENV)
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("{SID_ENV} is not set or not UTF-8"))?;
        if !valid_sid(sid) {
            return Err(format!("{SID_ENV} is not a session id"));
        }
        let dir = control_dir(env).ok_or("neither XDG_RUNTIME_DIR nor an absolute HOME is set")?;
        Ok(Self {
            dir,
            sid: sid.to_string(),
        })
    }

    /// Read the socket this session's graph entry names now.
    ///
    /// # Errors
    /// The graph entry is missing, unsafe, malformed, or names no absolute socket.
    pub fn sock(&self) -> Result<PathBuf, String> {
        let entry = self.dir.join("graph").join(&self.sid);
        let body = read_regular_file(&entry, MAX_GRAPH_BYTES).map_err(|error| {
            format!(
                "no graph entry for {} at {} ({error})",
                self.sid,
                entry.display()
            )
        })?;
        graph_sock(&body).ok_or_else(|| format!("the graph entry for {} names no socket", self.sid))
    }
}

/// The token-file candidates aterm's socket naming rule admits, in order.
#[must_use]
pub fn token_names(sock_name: &str) -> Vec<String> {
    if let Some(pid) = sock_name
        .strip_suffix(".sock")
        .and_then(|name| name.strip_prefix("aterm-"))
        .filter(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
    {
        return vec![format!("aterm-{pid}.token")];
    }
    let own = if Path::new(sock_name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sock"))
    {
        format!("{sock_name}.token")
    } else {
        format!("aterm-sock-{sock_name}.token")
    };
    vec![own, "aterm.token".to_string()]
}

fn read_regular_file(path: &Path, max: u64) -> io::Result<String> {
    use rustix::fs::{Mode, OFlags, open};
    let file = open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(std::fs::File::from)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds its bound",
        ));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
}

fn read_token_file(path: &Path) -> io::Result<String> {
    let raw = read_regular_file(path, MAX_TOKEN_BYTES)?;
    let token = raw.trim();
    if token.is_empty() || token.contains(['\r', '\n', ' ', '\t']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the token file is not one token",
        ));
    }
    Ok(token.to_string())
}

/// Read the token beside `sock` without following token symlinks.
///
/// # Errors
/// No admissible token file exists beside the socket.
pub fn read_token_beside(sock: &Path) -> io::Result<String> {
    let (Some(dir), Some(name)) = (sock.parent(), sock.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the socket path names no directory",
        ));
    };
    let name = std::fs::read_link(sock)
        .ok()
        .and_then(|target| target.file_name().map(std::ffi::OsStr::to_os_string))
        .unwrap_or_else(|| name.to_os_string());
    let mut missing = None;
    for candidate in token_names(&name.to_string_lossy()) {
        match read_token_file(&dir.join(candidate)) {
            Ok(token) => return Ok(token),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => return Err(error),
            Err(error) => {
                missing.get_or_insert(error);
            }
        }
    }
    Err(missing
        .unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no token beside the socket")))
}

/// One authenticated request connection to aterm.
#[derive(Debug)]
pub struct Conn {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    sid: String,
    /// The version aterm reported during the handshake.
    pub version: String,
}

impl Conn {
    /// Connect, authenticate, and read aterm's version.
    ///
    /// # Errors
    /// The socket, token, or handshake failed.
    pub fn open(sock: &Path, sid: &str, timeout: Duration) -> io::Result<Self> {
        let stream = UnixStream::connect(sock)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let token = read_token_beside(sock)?;
        let writer = stream.try_clone()?;
        let mut conn = Self {
            reader: BufReader::new(stream),
            writer,
            sid: sid.to_string(),
            version: String::new(),
        };
        conn.writer
            .write_all(format!("AUTH {token}\nversion\n").as_bytes())?;
        conn.writer.flush()?;
        let line = conn.line()?;
        match line.strip_prefix("OK ") {
            Some(version) => {
                conn.version = version.to_string();
                Ok(conn)
            }
            None => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("aterm refused the connection: {line}"),
            )),
        }
    }

    fn line(&mut self) -> io::Result<String> {
        let mut bytes = Vec::new();
        let read = (&mut self.reader)
            .take(MAX_LINE_BYTES.saturating_add(1))
            .read_until(b'\n', &mut bytes)?;
        if read == 0 || bytes.last() != Some(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "aterm closed the connection or exceeded the line bound",
            ));
        }
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Send one verb to this connection's session and read its one-line reply.
    ///
    /// # Errors
    /// The connection failed. An `ERR` reply is returned as a reply.
    pub fn ask(&mut self, verb: &str) -> io::Result<String> {
        if verb.contains(['\n', '\r']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a verb is one line",
            ));
        }
        self.writer
            .write_all(format!("@{} {verb}\n", self.sid).as_bytes())?;
        self.writer.flush()?;
        self.line()
    }
}

/// The subscription verb for a session's lifecycle events.
#[must_use]
pub fn subscribe_verb(sid: &str) -> String {
    format!("subscribe @{sid} events")
}

/// A managed account home selected by aterm's account pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The roster row's label.
    pub label: String,
    /// Its `KIMI_CODE_HOME`, or `None` for the machine's default home.
    pub home: Option<PathBuf>,
}

/// A restart aterm asked this harness to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restart {
    /// Why the restart was asked.
    pub cause: String,
    /// The account to resume on, for an account move or credential renewal.
    pub account: Option<Account>,
}

fn restart(mut words: std::str::SplitAsciiWhitespace<'_>) -> Option<Restart> {
    let cause = words.next()?.strip_prefix("cause=")?.to_string();
    let mut label = None;
    let mut home = None;
    for word in words {
        if let Some(value) = word.strip_prefix("to=") {
            if label.replace(value.to_string()).is_some() {
                return None;
            }
        } else {
            let value = word.strip_prefix("home=")?;
            if home.is_some() {
                return None;
            }
            home = Some((value != "-").then(|| PathBuf::from(pct_decode(value))));
        }
    }
    let account = match (label, home) {
        (Some(label), Some(home))
            if matches!(cause.as_str(), "account" | "account-ahead")
                && !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                && home.as_ref().is_none_or(|home| home.is_absolute()) =>
        {
            Some(Account { label, home })
        }
        (None, None) => None,
        _ => return None,
    };
    Some(Restart { cause, account })
}

/// What one nonblocking event-stream drain found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drained {
    /// A `harness-policy` event arrived.
    pub policy_moved: bool,
    /// A `restart cause=<why>` event arrived.
    pub restart: Option<Restart>,
}

/// A second connection subscribed to a session's events.
#[derive(Debug)]
pub struct Events {
    stream: UnixStream,
    pending: Vec<u8>,
}

impl Events {
    /// Authenticate and subscribe to session `sid`'s events.
    ///
    /// # Errors
    /// The connection, handshake, or subscription failed.
    pub fn open(sock: &Path, sid: &str, timeout: Duration) -> io::Result<Self> {
        let mut conn = Conn::open(sock, sid, timeout)?;
        conn.writer
            .write_all(format!("{}\n", subscribe_verb(sid)).as_bytes())?;
        conn.writer.flush()?;
        let ack = conn.line()?;
        let Some(count) = ack
            .strip_prefix("OK subscribe ")
            .and_then(|count| count.trim().parse::<usize>().ok())
        else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("aterm refused the subscription: {ack}"),
            ));
        };
        for _ in 0..count {
            let row = conn.line()?;
            if !row.starts_with("sub ") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("the subscription's ack named no target: {row}"),
                ));
            }
        }
        let Conn { reader, writer, .. } = conn;
        let pending = reader.buffer().to_vec();
        drop(reader);
        writer.set_nonblocking(true)?;
        Ok(Self {
            stream: writer,
            pending,
        })
    }

    /// Read all complete events currently available without blocking.
    ///
    /// # Errors
    /// aterm closed the event stream.
    pub fn drain(&mut self) -> io::Result<Drained> {
        let mut buf = [0_u8; 4096];
        let mut closed = false;
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(read) => {
                    self.pending.extend_from_slice(&buf[..read]);
                    if u64::try_from(self.pending.len()).unwrap_or(u64::MAX) > MAX_LINE_BYTES {
                        self.pending.clear();
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        let mut out = Drained::default();
        let complete = self
            .pending
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |last| last.saturating_add(1));
        for line in self
            .pending
            .get(..complete)
            .unwrap_or_default()
            .split(|byte| *byte == b'\n')
        {
            let line = String::from_utf8_lossy(line);
            let mut words = line.split_ascii_whitespace();
            if words.next() != Some("EVENT") {
                continue;
            }
            let _session = words.next();
            match words.next() {
                Some("harness-policy") => out.policy_moved = true,
                Some("restart") => {
                    if let Some(event) = restart(words) {
                        out.restart = Some(event);
                    }
                }
                _ => {}
            }
        }
        self.pending = self
            .pending
            .get(complete..)
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        if closed {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "aterm closed the event stream",
            ));
        }
        Ok(out)
    }
}

fn fields(reply: &str) -> BTreeMap<String, String> {
    reply
        .split_ascii_whitespace()
        .skip_while(|word| *word == "OK")
        .filter_map(|word| word.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn pct_decode(value: &str) -> String {
    let hex = |byte: u8| {
        char::from(byte)
            .to_digit(16)
            .and_then(|digit| u8::try_from(digit).ok())
    };
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(index + 1).copied().and_then(hex),
                bytes.get(index + 2).copied().and_then(hex),
            )
        {
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn switch(fields: &BTreeMap<String, String>, key: &str) -> Option<bool> {
    match fields.get(key).map(String::as_str) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

/// The parts of `harness policy` that govern an upgrade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Contracts the host serves.
    pub contracts: Vec<u32>,
    /// Whether supervision is effective for this session.
    pub effective: bool,
    /// Why the policy is effective or not.
    pub why: String,
    /// `[harness] upgrade`.
    pub upgrade: Option<bool>,
    /// `[harness] upgrade_ask`.
    pub upgrade_ask: Option<bool>,
    /// `[harness] relaunch`.
    pub relaunch: Option<bool>,
    /// `[harness] human_grace_s`.
    pub human_grace: Option<Duration>,
}

impl Policy {
    /// Parse one `harness policy` reply.
    ///
    /// # Errors
    /// The reply is not an `OK` response.
    pub fn parse(reply: &str) -> Result<Self, String> {
        if reply != "OK" && !reply.starts_with("OK ") {
            return Err(format!("harness policy answered {reply}"));
        }
        let fields = fields(reply);
        Ok(Self {
            contracts: fields
                .get("contract")
                .map(|value| {
                    value
                        .split(',')
                        .filter_map(|word| word.parse::<u32>().ok())
                        .collect()
                })
                .unwrap_or_default(),
            effective: fields.get("effective").is_some_and(|value| value == "on"),
            why: fields
                .get("why")
                .map_or_else(|| "-".to_string(), |value| pct_decode(value)),
            upgrade: switch(&fields, "upgrade"),
            upgrade_ask: switch(&fields, "upgrade_ask"),
            relaunch: switch(&fields, "relaunch"),
            human_grace: fields
                .get("human_grace_s")
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs),
        })
    }

    /// Whether the host serves contract `contract`.
    #[must_use]
    pub fn serves(&self, contract: u32) -> bool {
        self.contracts.contains(&contract)
    }
}

/// Whether `version` is safe as a claim's `vendor=` word.
#[must_use]
pub fn vendor_word(version: Option<&str>) -> Option<&str> {
    version.filter(|version| {
        !version.is_empty()
            && version.len() <= 64
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte))
    })
}

/// This harness's holder for `pid`.
#[must_use]
pub fn holder(pid: u32) -> String {
    format!("{HOLDER_PREFIX}{pid}")
}

/// The scoped claim verb for this harness and vendor build.
#[must_use]
pub fn claim_verb(pid: u32, version: &str, vendor: Option<&str>, login: Option<&str>) -> String {
    let mut verb = format!(
        "meta set supervisor {} contract={CONTRACT} scope={SCOPE} version={version}",
        holder(pid)
    );
    if let Some(vendor) = vendor_word(vendor) {
        verb.push_str(" vendor=");
        verb.push_str(vendor);
    }
    if let Some(label) = login.filter(|label| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    }) {
        verb.push_str(" login=");
        verb.push_str(label);
    }
    verb
}

/// When a person last typed, as `status human_ms=` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Human {
    /// Nobody has typed in this session.
    Never,
    /// The last input was this long ago.
    Ago(Duration),
    /// No valid reading.
    Unknown,
}

/// The parts of `status` that witness a safe point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    /// The current supervisor holder.
    pub supervisor: Option<String>,
    /// Whether that claim is proven.
    pub proven: bool,
    /// `agent=`.
    pub agent: Option<String>,
    /// `composer=`.
    pub composer: Option<String>,
    /// `human_ms=`.
    pub human: Human,
    /// `hand=`.
    pub hand: Option<String>,
    /// `hold=`.
    pub hold: Option<String>,
    /// `input=`.
    pub input: Option<String>,
    /// `need=`.
    pub need: Option<String>,
}

impl Screen {
    /// Parse one schema-1 `status` reply.
    ///
    /// # Errors
    /// The reply is not `OK`, or its schema is not 1.
    pub fn parse(reply: &str) -> Result<Self, String> {
        if !reply.starts_with("OK ") {
            return Err(format!("status answered {reply}"));
        }
        let fields = fields(reply);
        let schema = fields
            .get("schema")
            .and_then(|value| value.split('.').next())
            .and_then(|value| value.parse::<u32>().ok());
        if schema != Some(1) {
            return Err("status is not schema 1".to_string());
        }
        let word = |key: &str| fields.get(key).map(|value| pct_decode(value));
        Ok(Self {
            supervisor: word("supervisor").filter(|value| value != "-"),
            proven: fields
                .get("supervisor_proven")
                .is_some_and(|value| value == "1"),
            agent: word("agent"),
            composer: word("composer"),
            human: match fields.get("human_ms").map(String::as_str) {
                Some("-") => Human::Never,
                Some(value) => value.parse::<u64>().map_or(Human::Unknown, |milliseconds| {
                    Human::Ago(Duration::from_millis(milliseconds))
                }),
                None => Human::Unknown,
            },
            hand: word("hand"),
            hold: word("hold"),
            input: word("input"),
            need: word("need"),
        })
    }

    /// Whether this screen proves `holder`'s claim.
    #[must_use]
    pub fn held_by(&self, holder: &str) -> bool {
        self.proven && self.supervisor.as_deref() == Some(holder)
    }
}

/// A proven claim and the connection it is bound to.
#[derive(Debug)]
pub struct Claimed {
    /// The claim's connection, kept open for its lifetime.
    pub conn: Conn,
    /// This harness's holder.
    pub holder: String,
    /// The most recently read policy.
    pub policy: Policy,
    /// Where the session can be reconnected.
    pub locator: Locator,
    /// The session's event stream, when aterm accepts the subscription.
    pub events: Option<Events>,
}

impl Claimed {
    /// Give this claim back while keeping the connection usable for a later claim.
    ///
    /// # Errors
    /// The connection failed or aterm refused the release.
    pub fn release(&mut self) -> Result<(), String> {
        let reply = self
            .conn
            .ask(&format!("meta unset supervisor holder={}", self.holder))
            .map_err(|error| error.to_string())?;
        if reply == "OK" || reply.starts_with("OK ") {
            Ok(())
        } else {
            Err(reply)
        }
    }

    /// Announce the claim again, including a new vendor version after a move.
    ///
    /// # Errors
    /// The connection failed or aterm refused the claim.
    pub fn reclaim(
        &mut self,
        pid: u32,
        version: &str,
        vendor: Option<&str>,
        login: Option<&str>,
    ) -> Result<(), String> {
        let reply = self
            .conn
            .ask(&claim_verb(pid, version, vendor, login))
            .map_err(|error| error.to_string())?;
        if reply == "OK" || reply.starts_with("OK ") {
            Ok(())
        } else {
            Err(reply)
        }
    }

    /// Claim and then require `status` to read the claim back as proven.
    ///
    /// # Errors
    /// The claim failed, or was not confirmed and was released again.
    pub fn claim_again(
        &mut self,
        pid: u32,
        version: &str,
        vendor: Option<&str>,
        login: Option<&str>,
    ) -> Result<(), String> {
        self.reclaim(pid, version, vendor, login)?;
        let screen = self.screen()?;
        if screen.held_by(&self.holder) {
            return Ok(());
        }
        let _ = self.release();
        Err(format!(
            "aterm did not confirm the claim (supervisor={} supervisor_proven={})",
            screen.supervisor.as_deref().unwrap_or("-"),
            u8::from(screen.proven)
        ))
    }

    /// Re-read `harness policy`.
    ///
    /// # Errors
    /// The request failed or the reply was not a policy.
    pub fn refresh_policy(&mut self) -> Result<&Policy, String> {
        let reply = self
            .conn
            .ask("harness policy")
            .map_err(|error| error.to_string())?;
        self.policy = Policy::parse(&reply)?;
        Ok(&self.policy)
    }

    /// Read `status`.
    ///
    /// # Errors
    /// The request failed or the reply was not a schema-1 status.
    pub fn screen(&mut self) -> Result<Screen, String> {
        let reply = self.conn.ask("status").map_err(|error| error.to_string())?;
        Screen::parse(&reply)
    }

    /// Require a fresh proven claim and policy permitting crash relaunch.
    ///
    /// # Errors
    /// The connection failed, the claim is not proven, or relaunch is disabled.
    pub fn relaunch_permitted(&mut self) -> Result<(), SafePointError> {
        let reply = self
            .conn
            .ask("harness policy")
            .map_err(|error| SafePointError::Io(error.to_string()))?;
        self.policy = Policy::parse(&reply).map_err(SafePointError::Hold)?;
        let reply = self
            .conn
            .ask("status")
            .map_err(|error| SafePointError::Io(error.to_string()))?;
        let screen = Screen::parse(&reply).map_err(SafePointError::Hold)?;
        if !screen.held_by(&self.holder) {
            return Err(SafePointError::Hold(
                "aterm does not show this harness's proven claim".to_string(),
            ));
        }
        if !self.policy.effective || !self.policy.serves(CONTRACT) {
            return Err(SafePointError::Hold(format!(
                "aterm's harness policy is off here (why={})",
                self.policy.why
            )));
        }
        match self.policy.relaunch {
            Some(true) => Ok(()),
            Some(false) => Err(SafePointError::Hold(
                "[harness] relaunch = false".to_string(),
            )),
            None => Err(SafePointError::Hold(
                "the policy reports no relaunch switch".to_string(),
            )),
        }
    }

    /// Take one fresh policy/status pair and judge every generic upgrade condition.
    ///
    /// # Errors
    /// A connection failed, or a safe-point condition did not hold.
    pub fn safe_point(&mut self) -> Result<(Policy, Screen), SafePointError> {
        let reply = self
            .conn
            .ask("harness policy")
            .map_err(|error| SafePointError::Io(error.to_string()))?;
        self.policy = Policy::parse(&reply).map_err(SafePointError::Hold)?;
        let reply = self
            .conn
            .ask("status")
            .map_err(|error| SafePointError::Io(error.to_string()))?;
        let screen = Screen::parse(&reply).map_err(SafePointError::Hold)?;
        screen_holds(&self.policy, &screen, &self.holder).map_err(SafePointError::Hold)?;
        Ok((self.policy.clone(), screen))
    }
}

/// Why a safe-point read did not pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafePointError {
    /// The claim's connection failed and must be reconnected.
    Io(String),
    /// The host answered, but its policy or screen does not permit a move.
    Hold(String),
}

/// Why one fresh policy/status pair is not an upgrade safe point.
///
/// # Errors
/// The claim, policy, screen, or input state does not meet contract 3's generic conditions.
pub fn screen_holds(policy: &Policy, screen: &Screen, holder: &str) -> Result<(), String> {
    if !screen.held_by(holder) {
        return Err(format!(
            "aterm does not show this harness's proven claim (supervisor={} supervisor_proven={})",
            screen.supervisor.as_deref().unwrap_or("-"),
            u8::from(screen.proven)
        ));
    }
    if !policy.effective || !policy.serves(CONTRACT) {
        return Err(format!(
            "aterm's harness policy is off here (why={})",
            policy.why
        ));
    }
    match policy.upgrade {
        Some(true) => {}
        Some(false) => return Err("[harness] upgrade = false".to_string()),
        None => return Err("the policy reports no upgrade switch".to_string()),
    }
    match policy.upgrade_ask {
        Some(false) => {}
        Some(true) => {
            return Err("[harness] upgrade_ask = true: a person says when (SIGUSR1)".to_string());
        }
        None => return Err("the policy reports no upgrade_ask switch".to_string()),
    }
    if screen.agent.as_deref() != Some("idle") {
        return Err(format!(
            "aterm reads agent={} (a move needs idle)",
            screen.agent.as_deref().unwrap_or("absent")
        ));
    }
    if !matches!(screen.composer.as_deref(), Some("empty" | "placeholder")) {
        return Err(format!(
            "aterm reads composer={} (a draft cannot be ruled out)",
            screen.composer.as_deref().unwrap_or("absent")
        ));
    }
    let grace = policy.human_grace.unwrap_or(Duration::from_secs(120));
    match screen.human {
        Human::Never => {}
        Human::Ago(ago) if ago >= grace => {}
        Human::Ago(ago) => {
            return Err(format!(
                "a person typed {} ms ago (the grace is {} ms)",
                ago.as_millis(),
                grace.as_millis()
            ));
        }
        Human::Unknown => return Err("aterm reports no human_ms".to_string()),
    }
    for (key, value, wanted) in [
        ("hand", screen.hand.as_deref(), "-"),
        ("hold", screen.hold.as_deref(), "0"),
        ("input", screen.input.as_deref(), "clear"),
        ("need", screen.need.as_deref(), "-"),
    ] {
        if value != Some(wanted) {
            return Err(format!("aterm reads {key}={}", value.unwrap_or("absent")));
        }
    }
    Ok(())
}

/// Claim a session under contract 3 and confirm the claim is proven.
///
/// # Errors
/// Why the harness must pass through to the vendor instead.
pub fn claim(
    locator: &Locator,
    pid: u32,
    version: &str,
    vendor: Option<&str>,
    login: Option<&str>,
    timeout: Duration,
) -> Result<Claimed, String> {
    let sock = locator.sock()?;
    let mut conn = Conn::open(&sock, &locator.sid, timeout).map_err(|error| {
        format!(
            "cannot open aterm's control socket {}: {error}",
            sock.display()
        )
    })?;
    let reply = conn
        .ask("harness policy")
        .map_err(|error| format!("harness policy: {error}"))?;
    let policy = Policy::parse(&reply)?;
    if !policy.serves(CONTRACT) {
        return Err(format!(
            "aterm does not serve harness contract {CONTRACT} (contract={})",
            if policy.contracts.is_empty() {
                "absent".to_string()
            } else {
                policy
                    .contracts
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
        ));
    }
    if !policy.effective {
        return Err(format!(
            "aterm's harness policy is off for this session (why={})",
            policy.why
        ));
    }
    let mut claimed = Claimed {
        conn,
        events: None,
        holder: holder(pid),
        policy,
        locator: locator.clone(),
    };
    claimed.claim_again(pid, version, vendor, login)?;
    claimed.events = Events::open(&sock, &locator.sid, timeout).ok();
    Ok(claimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env::from_pairs(pairs.iter().copied())
    }

    #[test]
    fn the_session_locator_and_socket_names_are_validated() {
        assert!(under_aterm(&env(&[("TERM_PROGRAM", "aterm")])));
        assert!(under_aterm(&env(&[("ATERM_CHILD", "1")])));
        assert!(left_out(&env(&[(NO_HARNESS_ENV, "1")])));
        assert!(valid_sid("s-00ff"));
        for bad in ["s-", "s-xyz", "../x", "s-1/2", "1f"] {
            assert!(!valid_sid(bad), "{bad}");
        }
        let locator =
            Locator::from_env(&env(&[(SID_ENV, "s-1a"), ("XDG_RUNTIME_DIR", "/run/u")])).unwrap();
        assert_eq!(locator.dir, PathBuf::from("/run/u/aterm"));
        assert_eq!(
            graph_sock("pid 1\nsock /a/b.sock\n"),
            Some(PathBuf::from("/a/b.sock"))
        );
        assert_eq!(graph_sock("sock relative.sock\n"), None);
        assert_eq!(token_names("aterm-77.sock"), ["aterm-77.token"]);
        assert_eq!(token_names("c.sock"), ["c.sock.token", "aterm.token"]);
    }

    #[test]
    fn policy_and_claim_use_contract_three_scoped_duties() {
        let policy = Policy::parse(
            "OK gen=4 effective=on why=enabled contract=2,3 upgrade=true upgrade_ask=false human_grace_s=90",
        )
        .unwrap();
        assert!(policy.serves(3) && policy.effective);
        assert_eq!(policy.human_grace, Some(Duration::from_secs(90)));
        assert!(!Policy::parse("OK contract=2").unwrap().serves(3));
        assert!(Policy::parse("ERR no").is_err());
        assert_eq!(
            claim_verb(412, "0.1.0", Some("2.5.1"), None),
            "meta set supervisor kimi-alab@412 contract=3 scope=relaunch,upgrade version=0.1.0 vendor=2.5.1"
        );
        assert!(
            claim_verb(412, "0.1.0", Some("2.5.1"), Some("work"))
                .ends_with(" vendor=2.5.1 login=work")
        );
        assert_eq!(vendor_word(Some("build 7")), None);
    }

    #[test]
    fn a_safe_point_needs_a_proven_idle_empty_unheld_session() {
        let policy = Policy::parse(
            "OK effective=on contract=2,3 upgrade=true upgrade_ask=false human_grace_s=120",
        )
        .unwrap();
        let screen = Screen::parse(
            "OK schema=1 supervisor=kimi-alab%40412 supervisor_proven=1 agent=idle composer=empty human_ms=- hand=- hold=0 input=clear need=-",
        )
        .unwrap();
        assert!(screen_holds(&policy, &screen, "kimi-alab@412").is_ok());
        let mut changed = screen.clone();
        changed.composer = Some("draft".to_string());
        assert!(screen_holds(&policy, &changed, "kimi-alab@412").is_err());
        let mut changed = screen.clone();
        changed.agent = Some("busy".to_string());
        assert!(screen_holds(&policy, &changed, "kimi-alab@412").is_err());
        let mut changed = screen;
        changed.proven = false;
        assert!(screen_holds(&policy, &changed, "kimi-alab@412").is_err());
        assert!(Screen::parse("OK schema=2").is_err());
        assert_eq!(pct_decode("a%20b%zz%4"), "a b%zz%4");
    }
}
