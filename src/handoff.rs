// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The inherited-descriptor handoff between two harness images during a self-re-exec. Schema 2
//! carries the child's kernel identity on an unlinked regular file. A schema-1 handoff has no
//! identity and is admitted only by independently capturing its direct live child at adoption.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::io::{FdFlags, fcntl_setfd};
use serde_json::{Map, Value};

use crate::aterm::Account;
use crate::proc::{Birth, ChildIdentity, Executable};
use crate::store::Build;

/// The descriptor whose bytes describe the running child to the next harness image.
pub const FD_ENV: &str = "KIMI_ALAB_EXEC_FD";
const SCHEMA: u64 = 2;
const MAX_BYTES: u64 = 64 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The live state that cannot be reconstructed from the store or the new image's argv.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// The vendor child's pid, still this process's child across `exec`.
    pub pid: i32,
    /// The vendor build that child runs.
    pub build: Build,
    /// The session its lock most recently proved.
    pub session: Option<String>,
    /// The account selected for that child.
    pub account: Option<Account>,
    /// How long the child has run.
    pub started_for: Duration,
    /// Whether its one immediate crash relaunch has been spent.
    pub crash_relaunched: bool,
    /// Vendor builds already found unable to start the session.
    pub abandoned: BTreeSet<u64>,
    /// Kernel proof of the live child. Absent only in a legacy schema-1 handoff; new execs require it.
    pub identity: Option<ChildIdentity>,
}

fn bytes(value: &OsStr) -> Value {
    Value::Array(
        value
            .as_bytes()
            .iter()
            .map(|byte| Value::from(u64::from(*byte)))
            .collect(),
    )
}

fn os(value: Option<&Value>) -> Result<OsString, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or("a handoff path is not bytes")?;
    let bytes = values
        .iter()
        .map(|byte| {
            byte.as_u64()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or("a handoff path has a non-byte")
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OsString::from_vec(bytes))
}

fn optional_string(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err("a handoff optional string has the wrong type".to_string()),
    }
}

fn identity_json(identity: &ChildIdentity) -> Value {
    let mut birth = Map::new();
    match identity.birth {
        Birth::MacOs {
            seconds,
            microseconds,
        } => {
            birth.insert("system".to_string(), Value::from("macos"));
            birth.insert("seconds".to_string(), Value::from(seconds));
            birth.insert("microseconds".to_string(), Value::from(microseconds));
        }
        Birth::Linux(ticks) => {
            birth.insert("system".to_string(), Value::from("linux"));
            birth.insert("ticks".to_string(), Value::from(ticks));
        }
    }
    let mut executable = Map::new();
    executable.insert(
        "path".to_string(),
        bytes(identity.executable.path.as_os_str()),
    );
    executable.insert(
        "device".to_string(),
        Value::from(identity.executable.device),
    );
    executable.insert("inode".to_string(), Value::from(identity.executable.inode));
    let mut root = Map::new();
    root.insert("pid".to_string(), Value::from(identity.pid));
    root.insert("parent".to_string(), Value::from(identity.parent));
    root.insert("pgid".to_string(), Value::from(identity.pgid));
    root.insert("session".to_string(), Value::from(identity.session));
    root.insert("birth".to_string(), Value::Object(birth));
    root.insert("executable".to_string(), Value::Object(executable));
    Value::Object(root)
}

fn encode(state: &State) -> Result<Vec<u8>, String> {
    let mut build = Map::new();
    build.insert("id".to_string(), Value::from(state.build.id));
    build.insert("dir".to_string(), bytes(state.build.dir.as_os_str()));
    build.insert("exe".to_string(), bytes(state.build.exe.as_os_str()));
    build.insert(
        "version".to_string(),
        state
            .build
            .version
            .as_ref()
            .map_or(Value::Null, |version| Value::String(version.clone())),
    );
    build.insert(
        "shim_env".to_string(),
        Value::Array(
            state
                .build
                .shim_env
                .iter()
                .map(|(key, value)| {
                    Value::Array(vec![
                        Value::String(key.clone()),
                        Value::String(value.clone()),
                    ])
                })
                .collect(),
        ),
    );
    let build = Value::Object(build);
    let account = state.account.as_ref().map_or(Value::Null, |account| {
        let mut row = Map::new();
        row.insert("label".to_string(), Value::String(account.label.clone()));
        row.insert(
            "home".to_string(),
            account
                .home
                .as_ref()
                .map_or(Value::Null, |home| bytes(home.as_os_str())),
        );
        Value::Object(row)
    });
    let identity = state
        .identity
        .as_ref()
        .ok_or("a new harness handoff has no kernel child identity")?;
    let mut root = Map::new();
    root.insert("schema".to_string(), Value::from(SCHEMA));
    root.insert("pid".to_string(), Value::from(state.pid));
    root.insert("build".to_string(), build);
    root.insert(
        "session".to_string(),
        state
            .session
            .as_ref()
            .map_or(Value::Null, |session| Value::String(session.clone())),
    );
    root.insert("account".to_string(), account);
    root.insert(
        "started_ms".to_string(),
        Value::from(u64::try_from(state.started_for.as_millis()).unwrap_or(u64::MAX)),
    );
    root.insert(
        "crash_relaunched".to_string(),
        Value::from(state.crash_relaunched),
    );
    root.insert(
        "abandoned".to_string(),
        Value::Array(state.abandoned.iter().copied().map(Value::from).collect()),
    );
    root.insert("identity".to_string(), identity_json(identity));
    let encoded = serde_json::to_vec(&Value::Object(root)).map_err(|error| error.to_string())?;
    if u64::try_from(encoded.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return Err("the harness handoff is too large".to_string());
    }
    Ok(encoded)
}

fn shim_env(build: &Map<String, Value>) -> Result<Vec<(String, String)>, &'static str> {
    build
        .get("shim_env")
        .and_then(Value::as_array)
        .ok_or("the handoff has no shim environment")?
        .iter()
        .map(|entry| {
            let pair = entry
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or("a handoff shim entry is not a pair")?;
            let key = pair
                .first()
                .and_then(Value::as_str)
                .ok_or("a shim key is not a string")?;
            let value = pair
                .get(1)
                .and_then(Value::as_str)
                .ok_or("a shim value is not a string")?;
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

fn u32_field(row: &Map<String, Value>, key: &str) -> Result<u32, String> {
    row.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("the handoff identity has no valid {key}"))
}

fn u64_field(row: &Map<String, Value>, key: &str) -> Result<u64, String> {
    row.get(key)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("the handoff identity has no valid {key}"))
}

fn decode_identity(
    value: Option<&Value>,
    pid: i32,
    schema: u64,
) -> Result<Option<ChildIdentity>, String> {
    if schema == 1 {
        return Ok(None);
    }
    let row = value
        .and_then(Value::as_object)
        .ok_or("the handoff has no kernel child identity")?;
    let identity_pid = u32_field(row, "pid")?;
    if i32::try_from(identity_pid).ok() != Some(pid) {
        return Err("the handoff identity names a different child".to_string());
    }
    let birth = row
        .get("birth")
        .and_then(Value::as_object)
        .ok_or("the handoff identity has no process birth")?;
    let birth = match birth.get("system").and_then(Value::as_str) {
        Some("macos") => {
            let seconds = u64_field(birth, "seconds")?;
            let microseconds = birth
                .get("microseconds")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value < 1_000_000)
                .ok_or("the handoff has invalid process birth microseconds")?;
            Birth::MacOs {
                seconds,
                microseconds,
            }
        }
        Some("linux") => Birth::Linux(u64_field(birth, "ticks")?),
        _ => return Err("the handoff has an unknown process birth system".to_string()),
    };
    let executable = row
        .get("executable")
        .and_then(Value::as_object)
        .ok_or("the handoff identity has no executable")?;
    let path = PathBuf::from(os(executable.get("path"))?);
    if !path.is_absolute() {
        return Err("the handoff identity has a relative executable path".to_string());
    }
    Ok(Some(ChildIdentity {
        pid: identity_pid,
        parent: u32_field(row, "parent")?,
        pgid: u32_field(row, "pgid")?,
        session: u32_field(row, "session")?,
        birth,
        executable: Executable {
            path,
            device: u64_field(executable, "device")?,
            inode: u64_field(executable, "inode")?,
        },
    }))
}

fn decode(text: &str) -> Result<State, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let root = value.as_object().ok_or("the handoff is not an object")?;
    let schema = root
        .get("schema")
        .and_then(Value::as_u64)
        .filter(|schema| matches!(*schema, 1 | SCHEMA))
        .ok_or("the handoff has an unknown schema")?;
    let pid = root
        .get("pid")
        .and_then(Value::as_i64)
        .and_then(|pid| i32::try_from(pid).ok())
        .filter(|pid| *pid > 1)
        .ok_or("the handoff names no child pid")?;
    let build = root
        .get("build")
        .and_then(Value::as_object)
        .ok_or("the handoff names no vendor build")?;
    let id = build
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("the handoff names no vendor build id")?;
    let dir = PathBuf::from(os(build.get("dir"))?);
    let exe = PathBuf::from(os(build.get("exe"))?);
    if !dir.is_absolute() || !exe.is_absolute() {
        return Err("the handoff has a relative vendor path".to_string());
    }
    let shim_env = shim_env(build)?;
    let build = Build {
        id,
        dir,
        exe,
        version: optional_string(build.get("version"))?,
        shim_env,
    };
    let account = match root.get("account") {
        Some(Value::Null) => None,
        Some(Value::Object(account)) => {
            let label = account
                .get("label")
                .and_then(Value::as_str)
                .filter(|label| {
                    !label.is_empty()
                        && label.len() <= 128
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
                })
                .ok_or("the handoff has an invalid account label")?
                .to_string();
            let home = match account.get("home") {
                Some(Value::Null) => None,
                value => Some(PathBuf::from(os(value)?)),
            };
            if home.as_ref().is_some_and(|home| !home.is_absolute()) {
                return Err("the handoff has a relative account home".to_string());
            }
            Some(Account { label, home })
        }
        _ => return Err("the handoff account has the wrong type".to_string()),
    };
    let started_ms = root
        .get("started_ms")
        .and_then(Value::as_u64)
        .ok_or("the handoff has no child age")?;
    let crash_relaunched = root
        .get("crash_relaunched")
        .and_then(Value::as_bool)
        .ok_or("the handoff has no crash history")?;
    let abandoned = root
        .get("abandoned")
        .and_then(Value::as_array)
        .ok_or("the handoff has no abandoned-build set")?
        .iter()
        .map(|id| id.as_u64().ok_or("an abandoned build is not an id"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let identity = decode_identity(root.get("identity"), pid, schema)?;
    Ok(State {
        pid,
        build,
        session: optional_string(root.get("session"))?,
        account,
        started_for: Duration::from_millis(started_ms),
        crash_relaunched,
        abandoned,
        identity,
    })
}

fn transport(encoded: &[u8]) -> Result<File, String> {
    use rustix::fs::{Mode, OFlags, open, unlink};

    let dir = std::env::temp_dir();
    if !dir.is_absolute() {
        return Err("the harness handoff temporary directory is not absolute".to_string());
    }
    for _ in 0..32 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(
            "kimi-alab-handoff-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        match open(
            &path,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => {
                let mut file = File::from(fd);
                let result = (|| {
                    file.write_all(encoded).map_err(|error| error.to_string())?;
                    file.rewind().map_err(|error| error.to_string())?;
                    unlink(&path).map_err(|error| error.to_string())?;
                    fcntl_setfd(&file, FdFlags::empty()).map_err(|error| error.to_string())?;
                    Ok(file)
                })();
                if result.is_err() {
                    let _ = unlink(&path);
                }
                return result;
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("cannot allocate a unique harness handoff file".to_string())
}

/// Replace this harness with `target`, carrying `state` on one inherited descriptor and the
/// child's current launch words in argv. The child itself is neither spawned nor signalled.
/// Returns only when the replacement could not be executed; the old image then still runs.
pub fn exec(target: &Path, args: &[OsString], state: &State) -> std::io::Error {
    use std::os::unix::process::CommandExt as _;

    let attempt = || -> Result<(), String> {
        let encoded = encode(state)?;
        let file = transport(&encoded)?;
        let mut command = Command::new(target);
        command
            .arg("run")
            .arg("--")
            .args(args)
            .env(FD_ENV, file.as_raw_fd().to_string());
        let error = command.exec();
        Err(format!("{}: {error}", target.display()))
    };
    match attempt() {
        Ok(()) => std::io::Error::other("exec returned without an error"),
        Err(error) => std::io::Error::other(error),
    }
}

/// Take the handoff named by `env`, if this image was reached by [`exec`]. Its descriptor is
/// owned and closed by the reading file, never inherited by the vendor's next relaunch.
///
/// # Errors
/// The variable is malformed, its descriptor is not a bounded regular file, or its state is invalid.
pub fn receive(env: &crate::env::Env) -> Result<Option<State>, String> {
    let Some(raw) = env.get(FD_ENV) else {
        return Ok(None);
    };
    let fd = raw
        .to_str()
        .and_then(|raw| raw.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
        .ok_or("KIMI_ALAB_EXEC_FD is not a descriptor")?;
    let file = take_file(fd);
    if !file
        .metadata()
        .map_err(|error| format!("the harness handoff could not be inspected: {error}"))?
        .is_file()
    {
        return Err("the harness handoff is not a regular file".to_string());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the harness handoff could not be read: {error}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return Err("the harness handoff is too large".to_string());
    }
    let text = String::from_utf8(bytes).map_err(|_| "the harness handoff is not UTF-8")?;
    decode(&text).map(Some)
}

#[allow(unsafe_code)]
fn take_file(fd: i32) -> File {
    // SAFETY: `exec` creates this descriptor, clears its close-on-exec flag, and transfers its
    // sole ownership to the replacement image. `receive` is the only consumer of `FD_ENV`.
    unsafe { File::from_raw_fd(fd) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ChildIdentity {
        ChildIdentity {
            pid: 4242,
            parent: 4241,
            pgid: 4241,
            session: 4240,
            birth: Birth::MacOs {
                seconds: 1_700_000_000,
                microseconds: 123,
            },
            executable: Executable {
                path: PathBuf::from("/store/kimi/17/bin/kimi"),
                device: 17,
                inode: 42,
            },
        }
    }

    fn state() -> State {
        State {
            pid: 4242,
            build: Build {
                id: 17,
                dir: PathBuf::from("/store/kimi/17"),
                exe: PathBuf::from("/store/kimi/17/bin/kimi"),
                version: Some("2.3.2".to_string()),
                shim_env: vec![("SHIM".to_string(), "yes".to_string())],
            },
            session: Some("session-1".to_string()),
            account: Some(Account {
                label: "work-2".to_string(),
                home: Some(PathBuf::from(OsString::from_vec(vec![
                    b'/', b'h', b'o', b'm', b'e', 0xff,
                ]))),
            }),
            started_for: Duration::from_secs(90),
            crash_relaunched: true,
            abandoned: BTreeSet::from([9, 11]),
            identity: Some(identity()),
        }
    }

    #[test]
    fn the_handoff_preserves_child_identity_build_account_and_crash_episode() {
        let state = state();
        let encoded = encode(&state).unwrap();
        assert_eq!(
            decode(std::str::from_utf8(&encoded).unwrap()).unwrap(),
            state
        );
        let mut damaged = serde_json::from_slice::<Value>(&encoded).unwrap();
        damaged["account"]["label"] = Value::from("not a label");
        assert!(decode(&damaged.to_string()).is_err());
        let mut damaged = serde_json::from_slice::<Value>(&encoded).unwrap();
        damaged["identity"]["birth"]["seconds"] = Value::from(0);
        assert!(decode(&damaged.to_string()).is_err());
    }

    #[test]
    fn a_legacy_handoff_requires_fresh_identity_capture_at_adoption() {
        let encoded = encode(&state()).unwrap();
        let mut legacy = serde_json::from_slice::<Value>(&encoded).unwrap();
        legacy["schema"] = Value::from(1);
        legacy.as_object_mut().unwrap().remove("identity");
        let decoded = decode(&legacy.to_string()).unwrap();
        assert_eq!(decoded.identity, None);
    }

    #[test]
    fn the_transport_carries_more_than_a_small_pipe_without_a_reader() {
        let encoded = vec![b'x'; 32 * 1024];
        let mut file = transport(&encoded).unwrap();
        assert!(file.metadata().unwrap().is_file());
        let mut read = Vec::new();
        file.read_to_end(&mut read).unwrap();
        assert_eq!(read, encoded);
    }

    #[test]
    fn a_failed_exec_returns_to_the_old_image() {
        let _process = crate::proc::PROCESS_TEST_LOCK.lock().unwrap();
        let error = exec(
            Path::new("/definitely/not/a/kimi-alab-build"),
            &[OsString::from("--session"), OsString::from("session-1")],
            &state(),
        );
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert!(error.to_string().contains("/definitely/not/a"));
        let mut legacy = state();
        legacy.identity = None;
        let error = exec(Path::new("/unused"), &[], &legacy);
        assert!(error.to_string().contains("no kernel child identity"));
    }
}
