//! Writing a verified contract out as a Foundry project the prover can build.
//!
//! Sources keep their source-unit paths at the project root, so the contract's own imports and
//! remappings still resolve. The test libraries come from one shared directory.
//!
//! Every value written here comes from whoever verified the contract. A source path could try to
//! climb out of the project, and a remapping could try to close its string and switch on
//! Foundry's FFI; so paths are reduced to plain components, strings are escaped, and the
//! compiler and EVM versions must look like what they are.

use crate::verified::VerifiedSource;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// Where the shared test libraries live on the build machine.
#[derive(Clone, Debug)]
pub struct Libraries {
    /// The directory Foundry may read outside the project.
    pub root: PathBuf,
}

impl Default for Libraries {
    fn default() -> Self {
        Self { root: PathBuf::from("/opt/lib") }
    }
}

/// The oldest compiler the current forge-std accepts. Older contracts build against the legacy
/// set under `<root>/legacy`: forge-std 1.9.7 and halmos-cheatcodes with its pragma lowered.
const CURRENT_LIBRARIES_FROM: Version = (0, 8, 13);
/// The oldest compiler the legacy set builds with (measured: 0.6.9 fails inside forge-std).
pub const OLDEST_COMPILER: Version = (0, 6, 10);

/// A solc version as (major, minor, patch).
type Version = (u32, u32, u32);

impl Libraries {
    fn remappings(&self, compiler: Version) -> [String; 2] {
        let root = self.root.display();
        let set = if compiler < CURRENT_LIBRARIES_FROM { format!("{root}/legacy") } else { root.to_string() };
        [format!("forge-std/={set}/forge-std/src/"), format!("halmos-cheatcodes/={set}/halmos-cheatcodes/src/")]
    }
}

/// Why a project could not be written.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// A source path leaves the project or names nothing.
    #[error("unsafe source path {0:?}")]
    UnsafePath(String),
    /// A compiler or EVM version is not in the expected form.
    #[error("unexpected {what} {value:?}")]
    BadSetting {
        /// Which setting.
        what: &'static str,
        /// Its value.
        value: String,
    },
    /// The contract's compiler is older than any test library builds with.
    #[error("solc {0} is older than {1}, the oldest the test libraries build with")]
    UnsupportedCompiler(String, String),
    /// Writing a file failed.
    #[error("writing {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: std::io::Error,
    },
}

/// Files the tools read from the project root. A source by one of these names would configure the
/// build or the prover instead of being compiled (a `halmos.toml` that does not parse stops every
/// proof), so it is refused.
const TOOL_FILES: [&str; 5] = ["foundry.toml", "halmos.toml", "remappings.txt", ".env", ".gitmodules"];

/// The path of a source file inside the project: plain components only. A leading slash is
/// dropped, as source-unit paths are often absolute paths from the author's machine.
///
/// # Errors
/// When the path climbs out of the project (`..`), names nothing, or names a tool file at the root.
pub fn source_path(project: &Path, unit: &str) -> Result<PathBuf, ProjectError> {
    let mut inside = PathBuf::new();
    for component in Path::new(unit.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => inside.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ProjectError::UnsafePath(unit.to_string()));
            }
        }
    }
    let tool_file =
        inside.components().count() == 1 && TOOL_FILES.iter().any(|name| inside.as_os_str().eq_ignore_ascii_case(name));
    if inside.as_os_str().is_empty() || tool_file {
        return Err(ProjectError::UnsafePath(unit.to_string()));
    }
    Ok(project.join(inside))
}

/// A TOML basic string. JSON string escapes are valid TOML escapes.
fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string always serialises")
}

/// `0.8.22` from `v0.8.22+commit.4fc1097e`.
fn solc_version(full: &str) -> Result<Version, ProjectError> {
    let bad = || ProjectError::BadSetting { what: "compiler version", value: full.to_string() };
    let version = full.trim_start_matches('v').split('+').next().unwrap_or("");
    let parts: Vec<u32> = version
        .split('.')
        .map(|part| if part.bytes().all(|b| b.is_ascii_digit()) { part.parse().ok() } else { None })
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    match parts[..] {
        [major, minor, patch] => Ok((major, minor, patch)),
        _ => Err(bad()),
    }
}

fn dotted((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

/// The verified remappings the project keeps: a `context:` prefix is dropped, entries without a
/// target are skipped, and the contract's own test libraries give way to the shared ones.
fn kept_remappings(verified: &[String]) -> impl Iterator<Item = &str> {
    verified
        .iter()
        .map(|remap| match (remap.find(':'), remap.find('=')) {
            (Some(colon), Some(equals)) if colon < equals => &remap[colon + 1..],
            _ => remap.as_str(),
        })
        .filter(|remap| remap.contains('='))
        .filter(|remap| !["forge-std/", "halmos-cheatcodes/", "ds-test/"].iter().any(|own| remap.starts_with(own)))
}

/// The `foundry.toml` for a verified contract.
///
/// # Errors
/// When the compiler or EVM version is not in the expected form.
pub fn foundry_toml(source: &VerifiedSource, libraries: &Libraries) -> Result<String, ProjectError> {
    let settings = &source.settings;
    let version = solc_version(&source.compiler_version)?;
    if version < OLDEST_COMPILER {
        return Err(ProjectError::UnsupportedCompiler(dotted(version), dotted(OLDEST_COMPILER)));
    }
    let mut toml = String::new();
    let lines = [
        "[profile.default]".to_string(),
        "src = \".\"".into(),
        "test = \"test\"".into(),
        "out = \"out\"".into(),
        "libs = []".into(),
        format!("allow_paths = [{}]", toml_string(&libraries.root.display().to_string())),
        "auto_detect_remappings = false".into(),
        format!("solc_version = \"{}\"", dotted(version)),
        format!("optimizer = {}", settings.optimizer_enabled),
        format!("optimizer_runs = {}", settings.optimizer_runs),
        format!("via_ir = {}", settings.via_ir),
    ];
    for line in lines {
        let _ = writeln!(toml, "{line}");
    }
    // "default", or nothing, leaves the choice to Foundry, which fits it to the solc version.
    if let Some(evm) = settings.evm_version.as_deref().filter(|evm| *evm != "default") {
        if !evm.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
            return Err(ProjectError::BadSetting { what: "EVM version", value: evm.to_string() });
        }
        let _ = writeln!(toml, "evm_version = \"{evm}\"");
    }
    toml.push_str("ffi = false\nremappings = [\n");
    for remap in libraries.remappings(version).iter().map(String::as_str).chain(kept_remappings(&settings.remappings)) {
        let _ = writeln!(toml, "  {},", toml_string(remap));
    }
    // The linter rejects Yul-only sources and adds nothing to a proof run.
    toml.push_str("]\n\n[lint]\nlint_on_build = false\n");
    Ok(toml)
}

/// Writes the sources and `foundry.toml` into `project`.
///
/// # Errors
/// See [`ProjectError`]; nothing is written when a path or setting is unsafe.
pub fn write(project: &Path, source: &VerifiedSource, libraries: &Libraries) -> Result<(), ProjectError> {
    let toml = foundry_toml(source, libraries)?;
    let files: Vec<(PathBuf, &String)> = source
        .sources
        .iter()
        .map(|(unit, content)| source_path(project, unit).map(|path| (path, content)))
        .collect::<Result<_, _>>()?;
    for (path, content) in files {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        std::fs::write(&path, content).map_err(io_error(&path))?;
    }
    let toml_path = project.join("foundry.toml");
    std::fs::write(&toml_path, toml).map_err(io_error(&toml_path))
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io { path: path.to_path_buf(), source }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verified::{CompilerSettings, Origin};
    use std::collections::BTreeMap;

    fn price_feed(remappings: Vec<String>) -> VerifiedSource {
        VerifiedSource {
            sources: BTreeMap::from([("contracts/PriceFeed.sol".to_string(), "contract PriceFeed {}".to_string())]),
            compiler_version: "0.8.22+commit.4fc1097e".into(),
            settings: CompilerSettings {
                optimizer_enabled: true,
                optimizer_runs: 20000,
                via_ir: false,
                evm_version: Some("paris".into()),
                remappings,
            },
            fully_qualified_name: "contracts/PriceFeed.sol:PriceFeed".into(),
            abi: None,
            constructor_args: None,
            deployment: None,
            origin: Origin::Sourcify,
        }
    }

    /// The configuration the kill-test pipeline wrote for this contract, which compiled and proved.
    const PRICE_FEED_TOML: &str = "[profile.default]\nsrc = \".\"\ntest = \"test\"\nout = \"out\"\nlibs = []\n\
        allow_paths = [\"/opt/lib\"]\nauto_detect_remappings = false\nsolc_version = \"0.8.22\"\noptimizer = true\n\
        optimizer_runs = 20000\nvia_ir = false\nevm_version = \"paris\"\nffi = false\nremappings = [\n\
        \x20 \"forge-std/=/opt/lib/forge-std/src/\",\n\x20 \"halmos-cheatcodes/=/opt/lib/halmos-cheatcodes/src/\",\n]\n\n\
        [lint]\nlint_on_build = false\n";

    #[test]
    fn the_configuration_matches_the_one_that_proved() {
        assert_eq!(foundry_toml(&price_feed(vec![]), &Libraries::default()).unwrap(), PRICE_FEED_TOML);
        assert!(toml::from_str::<toml::Table>(PRICE_FEED_TOML).is_ok());
    }

    #[test]
    fn old_compilers_get_the_legacy_libraries_and_older_ones_are_refused() {
        let with = |version: &str| {
            let mut source = price_feed(vec![]);
            source.compiler_version = version.into();
            foundry_toml(&source, &Libraries::default())
        };
        let old = with("v0.6.12+commit.27d51765").unwrap();
        assert!(old.contains("solc_version = \"0.6.12\""));
        assert!(old.contains("\"forge-std/=/opt/lib/legacy/forge-std/src/\""));
        assert!(old.contains("\"halmos-cheatcodes/=/opt/lib/legacy/halmos-cheatcodes/src/\""));
        assert!(with("0.8.12+commit.f00d7308").unwrap().contains("/opt/lib/legacy/"));
        assert!(!with("0.8.13+commit.abaa5c0e").unwrap().contains("/legacy/"));
        assert!(matches!(with("0.6.9+commit.3e3065ac"), Err(ProjectError::UnsupportedCompiler(..))));
        assert!(matches!(with("0.8"), Err(ProjectError::BadSetting { .. })));
    }

    #[test]
    fn remappings_drop_contexts_and_own_test_libraries() {
        let remaps = ["ctx:@oz/=lib/oz/", "forge-std/=lib/forge-std/src/", "ds-test/=lib/ds/", "noTarget", "@a/=b/"]
            .map(String::from);
        let kept: Vec<&str> = kept_remappings(&remaps).collect();
        assert_eq!(kept, ["@oz/=lib/oz/", "@a/=b/"]);
    }

    #[test]
    fn hostile_settings_cannot_reach_the_configuration() {
        let hostile = "a/=b/\"\nffi = true\n#=";
        let text = foundry_toml(&price_feed(vec![hostile.into()]), &Libraries::default()).unwrap();
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        let profile = parsed["profile"]["default"].as_table().unwrap();
        assert_eq!(profile["ffi"].as_bool(), Some(false));
        let remappings: Vec<&str> =
            profile["remappings"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(remappings.contains(&hostile), "the hostile remapping stays one string");

        let mut bad_version = price_feed(vec![]);
        bad_version.compiler_version = "0.8.22\"\nffi = true".into();
        assert!(foundry_toml(&bad_version, &Libraries::default()).is_err());
        let mut bad_evm = price_feed(vec![]);
        bad_evm.settings.evm_version = Some("paris\"\nffi=true".into());
        assert!(foundry_toml(&bad_evm, &Libraries::default()).is_err());
    }

    #[test]
    fn source_paths_stay_inside_the_project() {
        let root = Path::new("/work/p");
        assert_eq!(source_path(root, "contracts/A.sol").unwrap(), root.join("contracts/A.sol"));
        assert_eq!(source_path(root, "/Users/dev/x/A.sol").unwrap(), root.join("Users/dev/x/A.sol"));
        assert_eq!(source_path(root, "./a/./B.sol").unwrap(), root.join("a/B.sol"));
        assert!(source_path(root, "../../root/.ssh/authorized_keys").is_err());
        assert!(source_path(root, "a/../../b.sol").is_err());
        assert!(source_path(root, "/").is_err());
        assert!(source_path(root, "").is_err());
        // A source cannot stand in for a tool's own file at the root, but may share its name deeper.
        for tool in ["halmos.toml", "/foundry.toml", "./Remappings.txt", ".env"] {
            assert!(source_path(root, tool).is_err(), "{tool}");
        }
        assert!(source_path(root, "docs/halmos.toml").is_ok());
    }

    #[test]
    fn writing_refuses_before_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut hostile = price_feed(vec![]);
        hostile.sources.insert("../escape.sol".into(), "x".into());
        assert!(write(dir.path(), &hostile, &Libraries::default()).is_err());
        assert!(!dir.path().join("contracts").exists());
        write(dir.path(), &price_feed(vec![]), &Libraries::default()).unwrap();
        assert!(dir.path().join("contracts/PriceFeed.sol").is_file());
        assert!(dir.path().join("foundry.toml").is_file());
    }
}
