// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Finding the vendor CLI in the atpkg store — never on `PATH`, never through the `agents/`
//! twin — so the harness can never run itself, a stale copy, or a binary atpkg did not verify.
//!
//! The layout and its rules are aterm's, read at aterm 5e2c6d0c28:
//! - prefix: `crates/atpkg/src/platform/unix.rs:22-34` (`default_prefix`) and
//!   `crates/atpkg/src/store.rs:1548-1600` (`resolve`, `vet_prefix`);
//! - `store/<program>/current -> <build>`: `store.rs` `Layout::program_current`;
//! - `store/<program>/<build>/bin/<command>`: `crates/atpkg/src/vendor_direct/lane.rs:1138`;
//! - `<build>.ready`: `store.rs:366-509` (`build_is_complete` and its helpers);
//! - `<build>.shim-env`: `crates/atpkg/src/shim_env.rs` (`ShimEnv::from_lines`, fail-closed);
//! - `<build>.vendor` `version = "…"`, and the build id ↔ version encoding:
//!   `crates/atpkg/src/vendor_direct/version.rs:1-140`.

use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::config;
use crate::env::Env;

/// The variable that moves the store for this harness alone (tests, unusual installs). It must
/// be absolute; it wins over `aterm.toml`.
pub const PREFIX_ENV: &str = "KIMI_ALAB_PKG_PREFIX";

/// Where the prefix came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefixSource {
    /// [`PREFIX_ENV`].
    Env,
    /// `[packages] prefix` in this `aterm.toml`.
    Config(PathBuf),
    /// atpkg's default under `$HOME`.
    Default,
}

/// The resolved atpkg prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// The prefix directory.
    pub path: PathBuf,
    /// Where it came from.
    pub source: PrefixSource,
}

/// One installed vendor build, resolved and checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    /// The store build id (`store/<program>/<id>`).
    pub id: u64,
    /// `store/<program>/<id>`.
    pub dir: PathBuf,
    /// `store/<program>/<id>/bin/<command>`.
    pub exe: PathBuf,
    /// The vendor version the build's `.vendor` record names, only when it names this very
    /// build (the version the id encodes); `None` when the record is missing, unreadable or
    /// names another version. Never taken from the id alone ([`build_version`]).
    pub version: Option<String>,
    /// The environment atpkg's shim exports for this build (`<id>.shim-env`).
    pub shim_env: Vec<(String, String)>,
}

impl Build {
    /// The version for display: the recorded one, else `build <id>`.
    pub fn version_label(&self) -> String {
        self.version
            .clone()
            .unwrap_or_else(|| format!("build {}", self.id))
    }
}

/// Why no runnable vendor build was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// `$HOME` is unset or relative and no prefix was named.
    NoHome,
    /// [`PREFIX_ENV`] is set but not an absolute path without `..`.
    BadPrefixEnv(String),
    /// `aterm.toml` names a prefix the harness will not guess at.
    Config(String),
    /// `store/<program>/current` does not exist: the program is not installed.
    NotInstalled {
        /// The program.
        program: String,
        /// The `current` link that is missing.
        current: PathBuf,
    },
    /// The store is not laid out as atpkg lays it out.
    Layout(String),
    /// The build `current` names has no accepting `.ready` marker.
    Incomplete(PathBuf),
    /// The build has no regular, executable `bin/<command>`.
    NoBinary(PathBuf),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => write!(
                f,
                "$HOME is unset or not absolute, so the atpkg store cannot be found (set {PREFIX_ENV})"
            ),
            Self::BadPrefixEnv(v) => {
                write!(f, "{PREFIX_ENV}={v:?} is not an absolute path without `..`")
            }
            Self::Config(why) => write!(f, "{why}"),
            Self::NotInstalled { program, current } => write!(
                f,
                "{program} is not installed in the atpkg store ({} does not exist); install it with \
                 `aterm pkg install {program}` — kimi-alab never runs a `{program}` found on PATH",
                current.display()
            ),
            Self::Layout(why) => write!(
                f,
                "the atpkg store is not laid out as atpkg lays it out: {why}"
            ),
            Self::Incomplete(dir) => write!(
                f,
                "{} is not a complete install for this machine (its .ready marker does not accept it); \
                 run `aterm pkg doctor`",
                dir.display()
            ),
            Self::NoBinary(exe) => write!(f, "{} is not a regular executable file", exe.display()),
        }
    }
}

/// The atpkg prefix for this run: [`PREFIX_ENV`], else `aterm.toml`'s `[packages] prefix`, else
/// atpkg's default under `$HOME`.
///
/// # Errors
/// [`StoreError::BadPrefixEnv`], [`StoreError::Config`] or [`StoreError::NoHome`].
pub fn resolve_prefix(env: &Env) -> Result<Prefix, StoreError> {
    if let Some(v) = env.get(PREFIX_ENV) {
        let path = PathBuf::from(v);
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(StoreError::BadPrefixEnv(v.to_string_lossy().into_owned()));
        }
        return Ok(Prefix {
            path,
            source: PrefixSource::Env,
        });
    }
    let home = env.home();
    if let Some(cfg) = config::config_path(env)
        && let Some(path) =
            config::configured_prefix(&cfg, home.as_deref()).map_err(StoreError::Config)?
    {
        // atpkg's `vet_prefix` admits a prefix outside $HOME only through a trusted system
        // chain this harness does not re-derive: refuse it rather than run a different store.
        if !home.as_deref().is_some_and(|h| path.starts_with(h)) {
            return Err(StoreError::Config(format!(
                "{}: [packages] prefix {} is outside $HOME, which kimi-alab 0.1 does not vet; \
                 set {PREFIX_ENV} to use it",
                cfg.display(),
                path.display()
            )));
        }
        return Ok(Prefix {
            path,
            source: PrefixSource::Config(cfg),
        });
    }
    let home = home.ok_or(StoreError::NoHome)?;
    Ok(Prefix {
        path: default_prefix(&home),
        source: PrefixSource::Default,
    })
}

/// atpkg's default prefix under `home` (`crates/atpkg/src/platform/unix.rs:22-34`).
pub fn default_prefix(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/aterm/pkg")
    } else {
        home.join(".local/share/aterm/pkg")
    }
}

/// `<prefix>/store/<program>`.
pub fn program_dir(prefix: &Path, program: &str) -> PathBuf {
    prefix.join("store").join(program)
}

/// The build id `store/<program>/current` names right now, read without any other check — the
/// watcher's cheap poll. `None` when there is no readable link naming a decimal build.
pub fn current_id(prefix: &Path, program: &str) -> Option<u64> {
    let link = program_dir(prefix, program).join("current");
    let target = std::fs::read_link(link).ok()?;
    parse_build_id(target.file_name()?.to_str()?)
}

/// The build `store/<program>/current` names, checked as atpkg checks a build before it runs
/// one: the link is a symlink to a sibling decimal build directory, the build's `.ready` marker
/// accepts it on this machine, and `bin/<command>` is a regular executable file.
///
/// # Errors
/// Each [`StoreError`] names the first check that failed.
pub fn current(prefix: &Path, program: &str, command: &str) -> Result<Build, StoreError> {
    let pdir = program_dir(prefix, program);
    let link = pdir.join("current");
    let meta = match std::fs::symlink_metadata(&link) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(StoreError::NotInstalled {
                program: program.to_string(),
                current: link,
            });
        }
        Err(e) => return Err(StoreError::Layout(format!("{}: {e}", link.display()))),
    };
    if !meta.file_type().is_symlink() {
        return Err(StoreError::Layout(format!(
            "{} is not a symlink",
            link.display()
        )));
    }
    let target = std::fs::read_link(&link)
        .map_err(|e| StoreError::Layout(format!("{}: {e}", link.display())))?;
    let target = if target.is_absolute() {
        target
    } else {
        pdir.join(target)
    };
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| StoreError::Layout(format!("{} names no build", link.display())))?;
    let id = parse_build_id(name).ok_or_else(|| {
        StoreError::Layout(format!(
            "{} names {name:?}, not a decimal build id",
            link.display()
        ))
    })?;
    let real_pdir = std::fs::canonicalize(&pdir)
        .map_err(|e| StoreError::Layout(format!("{}: {e}", pdir.display())))?;
    let dir = std::fs::canonicalize(&target).map_err(|e| {
        StoreError::Layout(format!("{} -> {}: {e}", link.display(), target.display()))
    })?;
    if dir.parent() != Some(real_pdir.as_path()) || dir.file_name() != Some(name.as_ref()) {
        return Err(StoreError::Layout(format!(
            "{} points outside {}",
            link.display(),
            pdir.display()
        )));
    }
    build_at(&dir, id, command)
}

/// The build in `dir` (whose id is `id`), checked as [`current`] checks it.
///
/// # Errors
/// [`StoreError::Incomplete`] or [`StoreError::NoBinary`].
pub fn build_at(dir: &Path, id: u64, command: &str) -> Result<Build, StoreError> {
    if !std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) || !build_is_complete(dir) {
        return Err(StoreError::Incomplete(dir.to_path_buf()));
    }
    let bin = dir.join("bin");
    let exe = bin.join(command);
    let runnable = std::fs::symlink_metadata(&bin).is_ok_and(|m| m.is_dir())
        && std::fs::symlink_metadata(&exe)
            .is_ok_and(|m| m.file_type().is_file() && m.permissions().mode() & 0o111 != 0);
    if !runnable {
        return Err(StoreError::NoBinary(exe));
    }
    Ok(Build {
        id,
        dir: dir.to_path_buf(),
        exe,
        version: build_version(dir, id),
        shim_env: shim_env(dir),
    })
}

/// Whether `exe` is this harness's own executable (the same device and inode as
/// [`std::env::current_exe`]): a store entry that IS the harness (a hard link or the file itself)
/// must never be run as the vendor. A store entry that only LEADS BACK to the harness (a script or
/// a copy) is caught by [`crate::supervise::nesting`] in the harness it starts.
pub fn is_this_harness(exe: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let me = std::env::current_exe().and_then(std::fs::metadata);
    let it = std::fs::metadata(exe);
    matches!((me, it), (Ok(me), Ok(it)) if me.dev() == it.dev() && me.ino() == it.ino())
}

/// A store build id: decimal digits only, no sign, no leading zero (atpkg writes them so).
pub fn parse_build_id(name: &str) -> Option<u64> {
    if name.is_empty()
        || !name.bytes().all(|b| b.is_ascii_digit())
        || (name.len() > 1 && name.starts_with('0'))
    {
        return None;
    }
    name.parse().ok()
}

/// `<dir>.<suffix>`, the sidecar atpkg keeps beside a build directory.
fn sidecar(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let name = dir.file_name()?.to_str()?;
    Some(dir.with_file_name(format!("{name}.{suffix}")))
}

/// Read a regular file of at most `max` bytes from one open handle without blocking on FIFOs
/// or truncating.
pub(crate) fn read_bounded(path: &Path, max: u64) -> Option<String> {
    read_bounded_opts(path, max, true)
}

/// Read a store sidecar (≤ `max` bytes, regular, never a symlink) from one open handle.
fn read_sidecar(path: &Path, max: u64) -> Option<String> {
    read_bounded_opts(path, max, false)
}

fn read_bounded_opts(path: &Path, max: u64, follow: bool) -> Option<String> {
    use rustix::fs::{Mode, OFlags, open};
    use std::io::Read as _;
    let mut flags = OFlags::RDONLY | OFlags::NONBLOCK;
    if !follow {
        flags |= OFlags::NOFOLLOW;
    }
    let f = std::fs::File::from(open(path, flags, Mode::empty()).ok()?);
    let meta = f.metadata().ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    let mut text = String::new();
    f.take(max.saturating_add(1))
        .read_to_string(&mut text)
        .ok()?;
    (u64::try_from(text.len()).ok()? <= max).then_some(text)
}

/// Whether `dir` holds a complete install for this machine (`store.rs:493-509`): the marker's
/// first line is `ok`; a `platform=` line, if any, names `<arch>-<os>` of this process; a
/// `contents=bin:a,b` line, if any, names only files still present in `bin/`.
pub fn build_is_complete(dir: &Path) -> bool {
    let Some(marker) = sidecar(dir, "ready") else {
        return false;
    };
    let Some(text) = read_sidecar(&marker, 64 * 1024) else {
        return false;
    };
    if text.lines().next().map(str::trim) != Some("ok") {
        return false;
    }
    let running = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let recorded = |key: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(key))
            .map(str::trim)
            .find(|v| !v.is_empty())
    };
    if recorded("platform=").is_some_and(|p| p != running) {
        return false;
    }
    if let Some(list) = recorded("contents=").and_then(|c| c.strip_prefix("bin:")) {
        let bin = dir.join("bin");
        let gone = list.split(',').filter(|n| !n.is_empty()).any(|n| {
            matches!(std::fs::symlink_metadata(bin.join(n)), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        });
        if gone {
            return false;
        }
    }
    true
}

/// `version = "…"` from `<dir>.vendor`, the record atpkg writes for a vendor-direct build.
pub fn recorded_version(dir: &Path) -> Option<String> {
    let text = read_sidecar(&sidecar(dir, "vendor")?, 64 * 1024)?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() != "version" {
            return None;
        }
        let v = v.trim().strip_prefix('"')?.strip_suffix('"')?;
        let ok = !v.is_empty()
            && v.len() <= 64
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b));
        ok.then(|| v.to_string())
    })
}

/// The vendor version of build `id` in `dir`: what its `.vendor` record names, only when that is
/// the version the id encodes, so the record and the directory agree before either is believed
/// (aterm's store reads a build the same way). A record that is missing, unreadable or names
/// another version gives `None`, and so does an id with no record: a version is never decoded
/// from a directory name alone.
pub fn build_version(dir: &Path, id: u64) -> Option<String> {
    let recorded = recorded_version(dir)?;
    (decode_vendor_build(id).as_deref() == Some(recorded.as_str())).then_some(recorded)
}

const VENDOR_BASE: u64 = 1_000_000_000_000_000_000;
const REVISION_BASE: u64 = 2 * VENDOR_BASE;
const REVISION_END: u64 = 3 * VENDOR_BASE;

/// The vendor version a vendor-direct build id encodes (`vendor_direct/version.rs`):
/// `10^18 + x·10^12 + y·10^6 + z` is `x.y.z`, and
/// `2·10^18 + x·10^15 + y·10^12 + z·10^9 + n·10^3 + s'` is `x.y.z-R<n>[.<s'-1>]`.
pub fn decode_vendor_build(id: u64) -> Option<String> {
    if id < REVISION_BASE
        && let Some(v) = id.checked_sub(VENDOR_BASE)
    {
        return Some(format!(
            "{}.{}.{}",
            v / 1_000_000_000_000,
            v / 1_000_000 % 1_000_000,
            v % 1_000_000
        ));
    }
    if id < REVISION_END
        && let Some(v) = id.checked_sub(REVISION_BASE)
    {
        let (major, minor, patch) = (
            v / 1_000_000_000_000_000 % 1_000,
            v / 1_000_000_000_000 % 1_000,
            v / 1_000_000_000 % 1_000,
        );
        let (rev, sub) = (v / 1_000 % 1_000_000, v % 1_000);
        return Some(match sub.checked_sub(1) {
            None => format!("{major}.{minor}.{patch}-R{rev}"),
            Some(point) => format!("{major}.{minor}.{patch}-R{rev}.{point}"),
        });
    }
    None
}

/// The environment `<dir>.shim-env` declares, admitted as atpkg admits it
/// (`shim_env.rs` `ShimEnv::from_lines`): any entry that breaks a rule drops the whole set.
pub fn shim_env(dir: &Path) -> Vec<(String, String)> {
    let Some(path) = sidecar(dir, "shim-env") else {
        return Vec::new();
    };
    read_sidecar(&path, 4096)
        .and_then(|t| admit_shim_env(&t))
        .unwrap_or_default()
}

/// Names a shim never sets (`shim_env.rs` `NEVER_SET`, `NEVER_SET_PREFIXES`).
const NEVER_SET: &[&str] = &[
    "PATH",
    "HOME",
    "IFS",
    "ENV",
    "BASH_ENV",
    "SHELLOPTS",
    "SHELL",
    "PWD",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "PATHEXT",
    "COMSPEC",
    "SYSTEMROOT",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PERL5OPT",
    "RUBYOPT",
];

/// The `NAME=VALUE` lines of a sidecar, or `None` if any breaks atpkg's rule.
pub fn admit_shim_env(text: &str) -> Option<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (name, value) = line.split_once('=')?;
        let name_ok = line.len() <= 256
            && !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            && !name.as_bytes()[0].is_ascii_digit()
            && !NEVER_SET.contains(&name)
            && !name.starts_with("LD_")
            && !name.starts_with("DYLD_");
        let value_ok = !value.is_empty()
            && !value.chars().any(char::is_control)
            && value.trim() == value
            && !value.contains('"')
            && !value.contains('%');
        if !name_ok || !value_ok || out.iter().any(|(n, _)| n == name) {
            return None;
        }
        out.push((name.to_string(), value.to_string()));
    }
    (out.len() <= 8).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A scratch directory under the target dir, removed on drop.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "kimi-alab-store-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn running() -> String {
        format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
    }

    fn lay_build(prefix: &Path, id: u64, ready: &str, exe_mode: Option<u32>) -> PathBuf {
        let dir = program_dir(prefix, "kimi").join(id.to_string());
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        if let Some(mode) = exe_mode {
            let exe = dir.join("bin/kimi");
            std::fs::write(&exe, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        std::fs::write(dir.with_file_name(format!("{id}.ready")), ready).unwrap();
        dir
    }

    fn point_current(prefix: &Path, id: u64) {
        let pdir = program_dir(prefix, "kimi");
        let link = pdir.join("current");
        let _ = std::fs::remove_file(&link);
        symlink(pdir.join(id.to_string()), link).unwrap();
    }

    #[test]
    fn a_missing_current_link_is_not_installed() {
        let s = Scratch::new("missing");
        let err = current(&s.0, "kimi", "kimi").unwrap_err();
        assert!(matches!(err, StoreError::NotInstalled { .. }), "{err:?}");
        assert!(err.to_string().contains("aterm pkg install kimi"));
        assert!(
            err.to_string()
                .contains("never runs a `kimi` found on PATH")
        );
    }

    #[test]
    fn current_resolves_a_ready_build_with_its_version_and_shim_env() {
        let s = Scratch::new("ready");
        let id = 1_000_002_000_003_000_001;
        let dir = lay_build(
            &s.0,
            id,
            &format!("ok\nplatform={}\ncontents=bin:kimi\n", running()),
            Some(0o755),
        );
        std::fs::write(
            dir.with_file_name(format!("{id}.vendor")),
            "schema = 1\nprogram = \"kimi\"\nversion = \"2.3.1\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.with_file_name(format!("{id}.shim-env")),
            "KIMI_CODE_NO_AUTO_UPDATE=1\n",
        )
        .unwrap();
        point_current(&s.0, id);
        let b = current(&s.0, "kimi", "kimi").unwrap();
        assert_eq!(b.id, id);
        assert!(b.exe.ends_with("bin/kimi"));
        assert_eq!(b.version.as_deref(), Some("2.3.1"));
        assert_eq!(
            b.shim_env,
            vec![("KIMI_CODE_NO_AUTO_UPDATE".to_string(), "1".to_string())]
        );
        assert_eq!(current_id(&s.0, "kimi"), Some(id));
    }

    #[test]
    fn the_version_is_the_record_only_where_it_names_the_build() {
        let s = Scratch::new("decode");
        let id = 1_000_002_000_003_000_002;
        let dir = lay_build(&s.0, id, "ok\n", Some(0o755));
        point_current(&s.0, id);
        let version = || current(&s.0, "kimi", "kimi").unwrap().version;
        // No record: the id encodes 2.3.2, but an id alone names no version.
        assert_eq!(version(), None);
        let record = dir.with_file_name(format!("{id}.vendor"));
        for (text, want) in [
            ("schema = 1\nversion = \"2.3.2\"\n", Some("2.3.2")),
            ("version = \"2.3.1\"\n", None),
            ("version = \"2.3.02\"\n", None),
            ("version = \"2.3.2-R1\"\n", None),
            ("version = 2.3.2\n", None),
            ("not a record\n", None),
            ("", None),
        ] {
            std::fs::write(&record, text).unwrap();
            assert_eq!(version().as_deref(), want, "{text:?}");
        }
    }

    #[test]
    fn an_incomplete_or_foreign_build_is_refused() {
        let s = Scratch::new("incomplete");
        lay_build(&s.0, 11, "staging\n", Some(0o755));
        point_current(&s.0, 11);
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::Incomplete(_))
        ));
        lay_build(&s.0, 12, "ok\nplatform=sparc-plan9\n", Some(0o755));
        point_current(&s.0, 12);
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::Incomplete(_))
        ));
        lay_build(&s.0, 13, "ok\ncontents=bin:kimi,kimi-helper\n", Some(0o755));
        point_current(&s.0, 13);
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::Incomplete(_))
        ));
    }

    #[test]
    fn a_missing_or_non_executable_binary_is_refused() {
        let s = Scratch::new("nobin");
        lay_build(&s.0, 21, "ok\n", None);
        point_current(&s.0, 21);
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::NoBinary(_))
        ));
        lay_build(&s.0, 22, "ok\n", Some(0o644));
        point_current(&s.0, 22);
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::NoBinary(_))
        ));
    }

    #[test]
    fn a_link_outside_the_program_dir_or_to_a_non_build_is_refused() {
        let s = Scratch::new("outside");
        let elsewhere = s.0.join("elsewhere/31");
        std::fs::create_dir_all(elsewhere.join("bin")).unwrap();
        std::fs::create_dir_all(program_dir(&s.0, "kimi")).unwrap();
        symlink(&elsewhere, program_dir(&s.0, "kimi").join("current")).unwrap();
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::Layout(_))
        ));
        let pdir = program_dir(&s.0, "kimi");
        std::fs::remove_file(pdir.join("current")).unwrap();
        std::fs::create_dir_all(pdir.join("latest")).unwrap();
        symlink(pdir.join("latest"), pdir.join("current")).unwrap();
        assert!(matches!(
            current(&s.0, "kimi", "kimi"),
            Err(StoreError::Layout(_))
        ));
    }

    #[test]
    fn build_ids_are_plain_decimals() {
        assert_eq!(
            parse_build_id("1000002000003000001"),
            Some(1_000_002_000_003_000_001)
        );
        assert_eq!(parse_build_id("0"), Some(0));
        for bad in ["", "01", "+1", "1a", "-3", "１"] {
            assert_eq!(parse_build_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn vendor_build_ids_decode_both_grammars() {
        assert_eq!(
            decode_vendor_build(1_000_002_000_001_000_310).as_deref(),
            Some("2.1.310")
        );
        assert_eq!(
            decode_vendor_build(1_000_001_000_004_000_007).as_deref(),
            Some("1.4.7")
        );
        assert_eq!(
            decode_vendor_build(2_001_004_013_006_996_002).as_deref(),
            Some("1.4.13-R6996.1")
        );
        assert_eq!(
            decode_vendor_build(2_001_004_013_006_996_000).as_deref(),
            Some("1.4.13-R6996")
        );
        assert_eq!(decode_vendor_build(15_698), None);
        assert_eq!(decode_vendor_build(VENDOR_BASE - 1), None);
        assert_eq!(decode_vendor_build(VENDOR_BASE).as_deref(), Some("0.0.0"));
        assert_eq!(
            decode_vendor_build(REVISION_BASE).as_deref(),
            Some("0.0.0-R0")
        );
        assert_eq!(decode_vendor_build(REVISION_END), None);
        assert_eq!(decode_vendor_build(u64::MAX), None);
    }

    #[test]
    fn shim_env_is_admitted_fail_closed() {
        assert_eq!(
            admit_shim_env("KIMI_CODE_NO_AUTO_UPDATE=1\n# note\n\nKIMI_DISABLE_TELEMETRY=1\n"),
            Some(vec![
                ("KIMI_CODE_NO_AUTO_UPDATE".into(), "1".into()),
                ("KIMI_DISABLE_TELEMETRY".into(), "1".into())
            ])
        );
        for bad in [
            "PATH=/x",
            "DYLD_INSERT_LIBRARIES=/x",
            "lower=1",
            "A=",
            "A=\"q\"",
            "A=1\nA=2",
            "1A=1",
            "NOEQUALS",
        ] {
            assert_eq!(admit_shim_env(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_env_override_must_be_absolute() {
        let env = Env::from_pairs([(PREFIX_ENV, "rel/pkg")]);
        assert!(matches!(
            resolve_prefix(&env),
            Err(StoreError::BadPrefixEnv(_))
        ));
        let env = Env::from_pairs([(PREFIX_ENV, "/x/../y")]);
        assert!(matches!(
            resolve_prefix(&env),
            Err(StoreError::BadPrefixEnv(_))
        ));
        let env = Env::from_pairs([(PREFIX_ENV, "/opt/pkg")]);
        assert_eq!(
            resolve_prefix(&env).unwrap().path,
            PathBuf::from("/opt/pkg")
        );
    }

    #[test]
    fn the_prefix_comes_from_config_then_the_default() {
        let s = Scratch::new("cfg");
        let home = s.0.join("home");
        let xdg = s.0.join("xdg");
        std::fs::create_dir_all(xdg.join("aterm")).unwrap();
        let env = Env::from_pairs([
            ("HOME", home.as_os_str()),
            ("XDG_CONFIG_HOME", xdg.as_os_str()),
        ]);
        assert_eq!(
            resolve_prefix(&env).unwrap(),
            Prefix {
                path: default_prefix(&home),
                source: PrefixSource::Default
            }
        );
        std::fs::write(
            xdg.join("aterm/aterm.toml"),
            "[packages]\nprefix = \"~/pkgs\"\n",
        )
        .unwrap();
        let p = resolve_prefix(&env).unwrap();
        assert_eq!(p.path, home.join("pkgs"));
        assert!(matches!(p.source, PrefixSource::Config(_)));
        std::fs::write(
            xdg.join("aterm/aterm.toml"),
            "[packages]\nprefix = \"/opt/elsewhere\"\n",
        )
        .unwrap();
        assert!(matches!(resolve_prefix(&env), Err(StoreError::Config(_))));
        std::fs::write(
            xdg.join("aterm/aterm.toml"),
            "[packages]\nprefix = 'unterminated\n",
        )
        .unwrap();
        assert!(matches!(resolve_prefix(&env), Err(StoreError::Config(_))));
        assert_eq!(resolve_prefix(&Env::default()), Err(StoreError::NoHome));
    }

    #[test]
    fn sidecars_are_read_from_one_handle_and_refuse_symlinks_and_fifos() {
        let _process = crate::proc::PROCESS_TEST_LOCK.lock().unwrap();
        let s = Scratch::new("sidecar");
        let file = s.0.join("1.ready");
        std::fs::write(&file, "ok\n").unwrap();
        assert_eq!(read_sidecar(&file, 16).as_deref(), Some("ok\n"));
        assert_eq!(read_sidecar(&file, 2), None);
        let link = s.0.join("2.ready");
        symlink(&file, &link).unwrap();
        assert_eq!(read_sidecar(&link, 16), None, "symlink sidecar refused");
        assert_eq!(read_bounded(&link, 16).as_deref(), Some("ok\n"));
        let fifo = s.0.join("3.ready");
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        assert_eq!(read_sidecar(&fifo, 16), None);
        assert_eq!(read_bounded(&fifo, 16), None);
    }
}
