// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Who the harness is: the one value a harness hands this library.
//!
//! Nothing in the library names a harness, a vendor or an environment variable
//! of its own. A harness builds one [`Config`] (its id, its vendor's word, the
//! duties it claims, its version and the contracts it declares) and, when it
//! is not aterm's default, the [`EnvNames`] it reads its session from, and
//! passes it in.

use std::ffi::OsString;
use std::fmt;

use crate::claim::{Contract, Scope, VERSION_MAX, claim_version};

/// The longest harness id or vendor word this library accepts.
const WORD_MAX: usize = 64;

/// Whether `word` is one lowercase word: `[a-z0-9][a-z0-9-]{0,63}`. A harness id
/// is the roster row, the atpkg package and the repository at once, and a
/// claim spells it before `@`, so it can hold none of `@`, `:`, `,` or space.
fn word_ok(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() <= WORD_MAX
        && bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// Which part of a [`Config`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The harness id is not one lowercase word.
    Id(String),
    /// The vendor word is not one lowercase word.
    Vendor(String),
    /// No duty is claimed.
    NoScope,
    /// No contract is declared ([`Config::with_contracts`]).
    NoContract,
    /// aterm would not read the version as a claim's `version=`.
    Version(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => write!(f, "harness id {id:?} is not [a-z0-9][a-z0-9-]{{0,63}}"),
            Self::Vendor(word) => {
                write!(f, "vendor word {word:?} is not [a-z0-9][a-z0-9-]{{0,63}}")
            }
            Self::NoScope => write!(f, "a claim takes at least one scope word"),
            Self::NoContract => write!(f, "a harness declares at least one contract"),
            Self::Version(version) => write!(
                f,
                "version {version:?} is not MAJOR.MINOR.0[+meta] of at most {VERSION_MAX} bytes"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// One process environment variable whose value says the process runs in an
/// aterm tab: `name` set to `value`, or set at all when `value` is `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedMark {
    /// The variable's name.
    pub name: String,
    /// The value it must hold, or `None` for any value.
    pub value: Option<String>,
}

/// The environment variables a harness reads its aterm session from.
/// [`EnvNames::default`] is aterm's own set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvNames {
    /// The session ID the harness's tab runs (`ATERM_PARENT_SESSION_ID`).
    pub session: String,
    /// The runtime directory that holds `aterm/graph/<sid>` (`XDG_RUNTIME_DIR`).
    pub runtime_dir: String,
    /// The home directory, whose `Library/Application Support/aterm` holds the
    /// graph when the runtime directory is unset (`HOME`).
    pub home: String,
    /// Any of these says the process runs in an aterm tab (`TERM_PROGRAM=aterm`,
    /// `ATERM_CHILD=1`, or `ATERM_SESSION_ID` set).
    pub hosted: Vec<HostedMark>,
}

impl Default for EnvNames {
    fn default() -> Self {
        let mark = |name: &str, value: Option<&str>| HostedMark {
            name: name.to_string(),
            value: value.map(str::to_string),
        };
        Self {
            session: "ATERM_PARENT_SESSION_ID".to_string(),
            runtime_dir: "XDG_RUNTIME_DIR".to_string(),
            home: "HOME".to_string(),
            hosted: vec![
                mark("TERM_PROGRAM", Some("aterm")),
                mark("ATERM_CHILD", Some("1")),
                mark("ATERM_SESSION_ID", None),
            ],
        }
    }
}

impl EnvNames {
    /// Whether `lookup` (a process environment) says the process runs in an
    /// aterm tab: any [`EnvNames::hosted`] mark holds.
    #[must_use]
    pub fn hosted_by(&self, lookup: impl Fn(&str) -> Option<OsString>) -> bool {
        self.hosted.iter().any(|mark| {
            lookup(&mark.name).is_some_and(|value| {
                mark.value
                    .as_deref()
                    .is_none_or(|wanted| value.as_os_str() == wanted)
            })
        })
    }

    /// [`EnvNames::hosted_by`] the process environment.
    #[must_use]
    pub fn hosted(&self) -> bool {
        self.hosted_by(|name| std::env::var_os(name))
    }
}

/// Who the harness is, as every claim and roster check spells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    id: String,
    vendor: String,
    scopes: Vec<Scope>,
    version: String,
    contracts: Vec<Contract>,
    env: EnvNames,
    stream: bool,
}

impl Config {
    /// The contracts a config declares until [`Config::with_contracts`] says
    /// otherwise: every contract this library speaks, 3 and 4. HX-14b deletes
    /// contract 3, and its place here with it.
    pub const DEFAULT_CONTRACTS: [Contract; 2] = [Contract::Three, Contract::Four];

    /// A harness `id` (its roster row, such as `kimi-alab`) running the vendor
    /// whose word is `vendor` (such as `kimi`), claiming `scopes` at its own
    /// `version`, declaring [`Config::DEFAULT_CONTRACTS`], with aterm's default
    /// [`EnvNames`].
    ///
    /// `scopes` are kept once each in aterm's order. They are the duties the
    /// harness can perform, and a claim asks for each one from aterm's
    /// built-in engine. What a claim names depends on the contract chosen at
    /// claim time ([`crate::claim::Contract::scopes`]): under contract 4 every
    /// word, under contract 3 only the words of [`Scope::CONTRACT_3`]
    /// (`relaunch`, `upgrade`), the only ones aterm's contract-3 roster rows
    /// allow. Either way the harness acts only on what aterm granted
    /// ([`crate::claim::Grant`]).
    ///
    /// # Errors
    /// `id` or `vendor` is not one lowercase word, `scopes` is empty, or
    /// `version` is not `MAJOR.MINOR.0[+meta]` (aterm refuses the whole claim
    /// over any of them).
    pub fn new(
        id: &str,
        vendor: &str,
        scopes: &[Scope],
        version: &str,
    ) -> Result<Self, ConfigError> {
        if !word_ok(id) {
            return Err(ConfigError::Id(id.to_string()));
        }
        if !word_ok(vendor) {
            return Err(ConfigError::Vendor(vendor.to_string()));
        }
        if scopes.is_empty() {
            return Err(ConfigError::NoScope);
        }
        if !claim_version(version) {
            return Err(ConfigError::Version(version.to_string()));
        }
        let mut scopes = scopes.to_vec();
        scopes.sort_unstable();
        scopes.dedup();
        Ok(Self {
            id: id.to_string(),
            vendor: vendor.to_string(),
            scopes,
            version: version.to_string(),
            contracts: Self::DEFAULT_CONTRACTS.to_vec(),
            env: EnvNames::default(),
            stream: true,
        })
    }

    /// This config claiming under `contracts` alone: the contracts the
    /// harness's record declares (`contracts =`; the publication plan's D11,
    /// under which every harness declares `[4]`), kept once each in
    /// ascending order. The link's every claim (at launch, from
    /// [`crate::Link::adopt_with`], and each made again after a loss or a
    /// policy switched back on) is made at the highest of them that aterm's
    /// policy lists ([`Contract::choose`]), and none is made while it lists
    /// none of them ([`crate::Failure::Unserved`]): a harness that declares
    /// `[4]` never claims 3, from an aterm that serves `2,3` at launch or
    /// after a restart.
    ///
    /// # Errors
    /// `contracts` is empty ([`ConfigError::NoContract`]): such a harness
    /// could never claim.
    pub fn with_contracts(mut self, contracts: &[Contract]) -> Result<Self, ConfigError> {
        if contracts.is_empty() {
            return Err(ConfigError::NoContract);
        }
        let mut contracts = contracts.to_vec();
        contracts.sort_unstable();
        contracts.dedup();
        self.contracts = contracts;
        Ok(self)
    }

    /// This config reading its session from `env` in place of aterm's names.
    #[must_use]
    pub fn with_env(mut self, env: EnvNames) -> Self {
        self.env = env;
        self
    }

    /// This config asking for the session stream once a contract-4 claim is
    /// granted (`true`, the default), or not: with `false` a contract-4 claim
    /// is held on the v0.1 path, renewed with `meta set supervisor` on the
    /// claim's connection and woken by the event subscription, and `stream` is
    /// never sent, for a harness that has not taken the stream yet and for a
    /// proof's control that claims and never opens one.
    #[must_use]
    pub const fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    /// Whether the link asks for the session stream ([`Config::with_stream`]).
    #[must_use]
    pub const fn streams(&self) -> bool {
        self.stream
    }

    /// The harness id: its roster row, atpkg package and repository.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The word of the vendor the harness runs.
    #[must_use]
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// The duties the harness asks for, once each in aterm's order.
    #[must_use]
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// The harness's own version, as its claim sends it.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The contracts the harness declares and claims under, once each in
    /// ascending order ([`Config::with_contracts`]).
    #[must_use]
    pub fn contracts(&self) -> &[Contract] {
        &self.contracts
    }

    /// The environment variables the harness reads its session from.
    #[must_use]
    pub fn env(&self) -> &EnvNames {
        &self.env
    }

    /// The claim holder for process `pid`: `<id>@<pid>`.
    #[must_use]
    pub fn holder(&self, pid: u32) -> String {
        crate::claim::holder(&self.id, pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn a_config_is_one_harness_its_vendor_its_duties_and_its_version() {
        let config = Config::new(
            "kimi-alab",
            "kimi",
            &[Scope::Upgrade, Scope::Relaunch, Scope::Upgrade],
            "0.4.0+g0dd6fe6",
        )
        .unwrap();
        assert_eq!(config.id(), "kimi-alab");
        assert_eq!(config.vendor(), "kimi");
        assert_eq!(config.scopes(), [Scope::Relaunch, Scope::Upgrade]);
        assert_eq!(config.version(), "0.4.0+g0dd6fe6");
        assert_eq!(config.holder(77), "kimi-alab@77");
        assert_eq!(config.env(), &EnvNames::default());
    }

    #[test]
    fn a_config_aterm_could_not_read_is_refused() {
        let new = |id: &str, vendor: &str, scopes: &[Scope], version: &str| {
            Config::new(id, vendor, scopes, version).map(|_| ())
        };
        for id in [
            "",
            "Kimi-alab",
            "-kimi",
            "kimi@alab",
            "kimi:alab",
            "kimi,alab",
            "kimi alab",
            "kimi_alab",
            &"a".repeat(65),
        ] {
            assert_eq!(
                new(id, "kimi", &[Scope::Upgrade], "0.1.0"),
                Err(ConfigError::Id(id.to_string())),
                "{id:?}"
            );
        }
        assert!(new(&"a".repeat(64), "kimi", &[Scope::Upgrade], "0.1.0").is_ok());
        assert_eq!(
            new("kimi-alab", "Kimi", &[Scope::Upgrade], "0.1.0"),
            Err(ConfigError::Vendor("Kimi".to_string()))
        );
        assert_eq!(
            new("kimi-alab", "kimi", &[], "0.1.0"),
            Err(ConfigError::NoScope)
        );
        // Every word is a duty a harness may ask for: the contract chosen at
        // claim time decides which a claim names, and aterm what it grants.
        assert_eq!(
            Config::new("kimi-alab", "kimi", &Scope::ALL, "0.1.0")
                .unwrap()
                .scopes(),
            Scope::ALL
        );
        assert_eq!(
            new("kimi-alab", "kimi", &[Scope::Upgrade], "0.1.1"),
            Err(ConfigError::Version("0.1.1".to_string()))
        );
        assert_eq!(
            ConfigError::Version("0.1.1".to_string()).to_string(),
            "version \"0.1.1\" is not MAJOR.MINOR.0[+meta] of at most 128 bytes"
        );
    }

    /// Until HX-14b deletes contract 3, a config declares both contracts this
    /// library speaks; a harness's record narrows them (D11's `[4]`), and a
    /// config that declares none is refused.
    #[test]
    fn the_default_contracts_are_three_and_four() {
        let config = Config::new("kimi-alab", "kimi", &[Scope::Upgrade], "0.1.0").unwrap();
        assert_eq!(config.contracts(), [Contract::Three, Contract::Four]);
        assert_eq!(Config::DEFAULT_CONTRACTS, [Contract::Three, Contract::Four]);
        let mut spoken = Contract::SPOKEN;
        spoken.sort_unstable();
        assert_eq!(Config::DEFAULT_CONTRACTS, spoken, "every contract spoken");
        let four = config
            .clone()
            .with_contracts(&[Contract::Four, Contract::Four])
            .unwrap();
        assert_eq!(four.contracts(), [Contract::Four]);
        assert_eq!((four.id(), four.scopes()), (config.id(), config.scopes()));
        assert_eq!(
            config
                .clone()
                .with_contracts(&[Contract::Four, Contract::Three])
                .unwrap()
                .contracts(),
            [Contract::Three, Contract::Four]
        );
        assert_eq!(config.with_contracts(&[]), Err(ConfigError::NoContract));
        assert_eq!(
            ConfigError::NoContract.to_string(),
            "a harness declares at least one contract"
        );
    }

    #[test]
    fn the_hosted_marks_are_aterms_and_can_be_replaced() {
        let names = EnvNames::default();
        assert!(names.hosted_by(lookup(&[("TERM_PROGRAM", "aterm")])));
        assert!(names.hosted_by(lookup(&[("ATERM_CHILD", "1")])));
        assert!(names.hosted_by(lookup(&[("ATERM_SESSION_ID", "")])));
        assert!(!names.hosted_by(lookup(&[("TERM_PROGRAM", "iTerm.app")])));
        assert!(!names.hosted_by(lookup(&[("ATERM_CHILD", "0")])));
        assert!(!names.hosted_by(lookup(&[])));
        let own = EnvNames {
            hosted: vec![HostedMark {
                name: "MY_HOST".to_string(),
                value: Some("yes".to_string()),
            }],
            ..EnvNames::default()
        };
        assert!(own.hosted_by(lookup(&[("MY_HOST", "yes")])));
        assert!(!own.hosted_by(lookup(&[("TERM_PROGRAM", "aterm")])));
        let config = Config::new("kimi-alab", "kimi", &[Scope::Upgrade], "0.1.0")
            .unwrap()
            .with_env(own.clone());
        assert_eq!(config.env(), &own);
    }

    #[test]
    fn the_locator_reads_the_variables_the_config_names() {
        let names = EnvNames::default();
        let locator = crate::wire::Locator::from_lookup(
            &names,
            lookup(&[
                ("ATERM_PARENT_SESSION_ID", "s-1a"),
                ("XDG_RUNTIME_DIR", "/run/u"),
            ]),
        )
        .unwrap();
        assert_eq!(locator.sid(), "s-1a");
        let own = EnvNames {
            session: "MY_SID".to_string(),
            runtime_dir: "MY_RUN".to_string(),
            home: "MY_HOME".to_string(),
            hosted: Vec::new(),
        };
        let pairs = [("MY_SID", "s-2b"), ("MY_HOME", "/h")];
        assert_eq!(
            crate::wire::Locator::from_lookup(&own, lookup(&pairs))
                .unwrap()
                .sid(),
            "s-2b"
        );
        // aterm's names are not read when the config names others.
        assert!(
            crate::wire::Locator::from_lookup(
                &own,
                lookup(&[("ATERM_PARENT_SESSION_ID", "s-1a"), ("HOME", "/h")])
            )
            .is_err()
        );
        for sid in ["", "1a", "s-xyz", "../s-1"] {
            assert!(
                crate::wire::Locator::from_lookup(
                    &names,
                    lookup(&[("ATERM_PARENT_SESSION_ID", sid), ("HOME", "/h")])
                )
                .is_err(),
                "{sid:?}"
            );
        }
    }
}
