use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    Confirmed,
    Probable,
    Unknown,
}

/// What an evidence item establishes about an ownership claim.
///
/// The variants deliberately describe semantics rather than detector names.
/// Confidence is derived from this closed set, so a detector cannot attach an
/// arbitrary confidence label to an otherwise weak claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceKind {
    PathEntry,
    SymlinkTarget,
    ResolvedTarget,
    ManagedPathLayout,
    OperatingSystemPath,
    PackageDatabaseOwnership,
    PackageDatabaseQuery,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PackageReceiptOwnership,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PackageReceiptMetadata,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    InstalledPackageReceipt,
    ManagerQueryMatch,
    ManagerQuery,
    Unconfirmed,
    ManagerLocation,
    ManagerLookup,
}

impl EvidenceKind {
    pub const fn source(self) -> &'static str {
        match self {
            Self::PathEntry => "PATH",
            Self::SymlinkTarget => "symlink",
            Self::ResolvedTarget => "filesystem",
            Self::ManagedPathLayout => "managed path",
            Self::OperatingSystemPath => "OS path",
            Self::PackageDatabaseOwnership | Self::PackageDatabaseQuery => "package query",
            Self::PackageReceiptOwnership
            | Self::PackageReceiptMetadata
            | Self::InstalledPackageReceipt => "pkgutil",
            Self::ManagerQueryMatch | Self::ManagerQuery => "manager query",
            Self::Unconfirmed => "unconfirmed",
            Self::ManagerLocation => "manager location",
            Self::ManagerLookup => "manager lookup",
        }
    }

    pub const fn confidence(self) -> Confidence {
        match self {
            Self::PackageDatabaseOwnership
            | Self::PackageReceiptOwnership
            | Self::ManagerQueryMatch => Confidence::Confirmed,
            Self::ManagedPathLayout | Self::OperatingSystemPath | Self::InstalledPackageReceipt => {
                Confidence::Probable
            }
            Self::PathEntry
            | Self::SymlinkTarget
            | Self::ResolvedTarget
            | Self::PackageDatabaseQuery
            | Self::PackageReceiptMetadata
            | Self::ManagerQuery
            | Self::Unconfirmed
            | Self::ManagerLocation
            | Self::ManagerLookup => Confidence::Unknown,
        }
    }
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Probable => "probable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerKind {
    PackageManager,
    VersionManager,
    Installer,
    ToolInstaller,
    OperatingSystem,
    Unknown,
}

impl OwnerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PackageManager => "package_manager",
            Self::VersionManager => "version_manager",
            Self::Installer => "installer",
            Self::ToolInstaller => "tool_installer",
            Self::OperatingSystem => "operating_system",
            Self::Unknown => "unknown",
        }
    }
}

// Defines OwnerId, its id/display_name/kind accessors, and the test-only
// ALL list from a single variant table below, instead of maintaining the
// enum and several derived matches as separate hand-written lists.
//
// Earlier revisions kept OwnerId and `ALL` as independent lists (or `ALL`
// derived from an auxiliary `next()` chain): both let a variant compile into
// as_str/display_name/kind without actually appearing in `ALL`, which
// silently weakens the identity tests for that variant. A macro that
// generates every one of these from the same token list makes that
// omission impossible: there is exactly one place to add an owner, and nothing
// else to keep in sync.
macro_rules! owner_ids {
    (
        $(
            $(#[$meta:meta])*
            $variant:ident { id: $id:literal, display: $display:literal, kind: $kind:expr }
        ),+ $(,)?
    ) => {
        /// Stable machine-facing identity of an owner.
        ///
        /// Ownership resolution, action guides, and detection rules branch on this
        /// value only. Display text is derived from it, never the other way around, so
        /// renaming an owner cannot change ownership-chain behavior.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum OwnerId {
            $($(#[$meta])* $variant),+
        }

        impl OwnerId {
            /// Stable snake_case identifier. Part of the machine-readable contract:
            /// changing one of these values is a breaking change for `--json` consumers.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $id),+
                }
            }

            /// Human-readable presentation text. Free to change without affecting
            /// ownership resolution or the machine-readable model.
            pub const fn display_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $display),+
                }
            }

            /// How the owner installs software. Derived from identity so an owner
            /// cannot be classified inconsistently between detection sites.
            pub const fn kind(self) -> OwnerKind {
                match self {
                    $(Self::$variant => $kind),+
                }
            }

            /// Every identity the tool can report, in declaration order.
            #[cfg(test)]
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
        }
    };
}

owner_ids! {
    Nix { id: "nix", display: "Nix", kind: OwnerKind::PackageManager },
    Homebrew { id: "homebrew", display: "Homebrew", kind: OwnerKind::PackageManager },
    Nvm { id: "nvm", display: "nvm", kind: OwnerKind::VersionManager },
    Fnm { id: "fnm", display: "fnm", kind: OwnerKind::VersionManager },
    Volta { id: "volta", display: "Volta", kind: OwnerKind::VersionManager },
    Mise { id: "mise", display: "mise", kind: OwnerKind::VersionManager },
    Asdf { id: "asdf", display: "asdf", kind: OwnerKind::VersionManager },
    Pyenv { id: "pyenv", display: "pyenv", kind: OwnerKind::VersionManager },
    Rbenv { id: "rbenv", display: "rbenv", kind: OwnerKind::VersionManager },
    Sdkman { id: "sdkman", display: "SDKMAN!", kind: OwnerKind::VersionManager },
    Uv { id: "uv", display: "uv", kind: OwnerKind::VersionManager },
    Rustup { id: "rustup", display: "rustup", kind: OwnerKind::VersionManager },
    RustupInstaller { id: "rustup_installer", display: "rustup installer", kind: OwnerKind::ToolInstaller },
    CargoInstall { id: "cargo_install", display: "cargo install", kind: OwnerKind::ToolInstaller },
    PnpmHome { id: "pnpm_home", display: "pnpm home", kind: OwnerKind::ToolInstaller },
    DenoInstaller { id: "deno_installer", display: "Deno installer", kind: OwnerKind::ToolInstaller },
    BunInstaller { id: "bun_installer", display: "Bun installer", kind: OwnerKind::ToolInstaller },
    // Installer receipts are only detected on macOS.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    MacosInstaller { id: "macos_installer", display: "macOS Installer (.pkg)", kind: OwnerKind::Installer },
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PythonOrgInstaller { id: "python_org_installer", display: "python.org macOS installer", kind: OwnerKind::Installer },
    MacPorts { id: "macports", display: "MacPorts", kind: OwnerKind::PackageManager },
    // Operating-system package queries only run on Linux.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Dpkg { id: "dpkg", display: "dpkg", kind: OwnerKind::PackageManager },
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Rpm { id: "rpm", display: "RPM", kind: OwnerKind::PackageManager },
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Pacman { id: "pacman", display: "pacman", kind: OwnerKind::PackageManager },
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Apk { id: "apk", display: "apk", kind: OwnerKind::PackageManager },
    OperatingSystem { id: "operating_system", display: "operating system", kind: OwnerKind::OperatingSystem },
    UnconfirmedOwner { id: "unconfirmed_owner", display: "unconfirmed owner", kind: OwnerKind::Unknown },
    UnconfirmedSource { id: "unconfirmed_source", display: "unconfirmed source", kind: OwnerKind::Unknown },
}

#[derive(Debug, Eq, PartialEq)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub detail: String,
}

impl Evidence {
    pub fn new(kind: EvidenceKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn source(&self) -> &'static str {
        self.kind.source()
    }

    pub fn confidence(&self) -> Confidence {
        self.kind.confidence()
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct ActionGuide {
    pub inspect: Option<String>,
    pub update: Option<String>,
    pub remove: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct OwnershipNode {
    pub id: OwnerId,
    pub package: Option<String>,
    pub version: Option<String>,
    pub evidence: Vec<Evidence>,
    pub actions: ActionGuide,
}

impl OwnershipNode {
    pub fn new(
        id: OwnerId,
        package: Option<String>,
        version: Option<String>,
        evidence: Vec<Evidence>,
        actions: ActionGuide,
    ) -> Self {
        Self {
            id,
            package,
            version,
            evidence,
            actions,
        }
    }

    pub fn display_name(&self) -> &'static str {
        self.id.display_name()
    }

    pub fn kind(&self) -> OwnerKind {
        self.id.kind()
    }

    pub fn confidence(&self) -> Confidence {
        if self
            .evidence
            .iter()
            .any(|evidence| evidence.confidence() == Confidence::Confirmed)
        {
            Confidence::Confirmed
        } else if self
            .evidence
            .iter()
            .any(|evidence| evidence.confidence() == Confidence::Probable)
        {
            Confidence::Probable
        } else {
            Confidence::Unknown
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionStatus {
    Active,
    Shadowed,
}

impl ResolutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Shadowed => "shadowed",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Resolution {
    pub path: PathBuf,
    pub real_path: PathBuf,
    pub status: ResolutionStatus,
    /// Ordered nearest-first: runtime -> owners[0] -> owners[1] -> ...
    pub owners: Vec<OwnershipNode>,
}

impl Resolution {
    pub fn primary_owner(&self) -> Option<&OwnershipNode> {
        self.owners.first()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct OwnershipGraph {
    pub command: String,
    pub resolutions: Vec<Resolution>,
}

impl OwnershipGraph {
    pub fn active(&self) -> Option<&Resolution> {
        self.resolutions
            .iter()
            .find(|resolution| resolution.status == ResolutionStatus::Active)
    }

    pub fn shadowed_count(&self) -> usize {
        self.resolutions
            .iter()
            .filter(|resolution| resolution.status == ResolutionStatus::Shadowed)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_owner_id_has_a_distinct_stable_snake_case_identifier() {
        let mut seen = Vec::new();
        for id in OwnerId::ALL {
            let value = id.as_str();
            assert!(
                !value.is_empty()
                    && value.chars().all(|character| character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '_'),
                "{value} is not a snake_case identifier"
            );
            assert!(!seen.contains(&value), "duplicate owner id {value}");
            seen.push(value);
        }
        assert_eq!(seen.len(), OwnerId::ALL.len());
    }

    #[test]
    fn stable_identifiers_are_independent_of_display_text() {
        assert_eq!(OwnerId::Sdkman.as_str(), "sdkman");
        assert_eq!(OwnerId::Sdkman.display_name(), "SDKMAN!");
        assert_eq!(OwnerId::MacosInstaller.as_str(), "macos_installer");
        assert_eq!(
            OwnerId::MacosInstaller.display_name(),
            "macOS Installer (.pkg)"
        );
        assert_eq!(OwnerId::Homebrew.as_str(), "homebrew");
        assert_eq!(OwnerId::Homebrew.display_name(), "Homebrew");
    }

    #[test]
    fn owner_kind_is_derived_from_identity_not_stored_per_node() {
        let node = OwnershipNode::new(OwnerId::Rustup, None, None, vec![], ActionGuide::default());
        assert_eq!(node.kind(), OwnerKind::VersionManager);
        assert_eq!(node.display_name(), "rustup");
        assert_eq!(OwnerId::RustupInstaller.kind(), OwnerKind::ToolInstaller);
    }

    #[test]
    fn confidence_is_derived_from_the_strongest_typed_evidence() {
        let cases = [
            (
                EvidenceKind::PackageDatabaseOwnership,
                Confidence::Confirmed,
            ),
            (EvidenceKind::ManagedPathLayout, Confidence::Probable),
            (EvidenceKind::Unconfirmed, Confidence::Unknown),
        ];

        for (kind, expected) in cases {
            let node = OwnershipNode::new(
                OwnerId::Homebrew,
                None,
                None,
                vec![Evidence::new(kind, "fixture")],
                ActionGuide::default(),
            );
            assert_eq!(node.confidence(), expected, "evidence kind: {kind:?}");
        }
    }

    #[test]
    fn confirming_evidence_outweighs_weaker_context() {
        let node = OwnershipNode::new(
            OwnerId::MacPorts,
            None,
            None,
            vec![
                Evidence::new(EvidenceKind::PathEntry, "fixture path"),
                Evidence::new(EvidenceKind::ManagedPathLayout, "MacPorts prefix"),
                Evidence::new(
                    EvidenceKind::PackageDatabaseOwnership,
                    "MacPorts registry owns the path",
                ),
            ],
            ActionGuide::default(),
        );

        assert_eq!(node.confidence(), Confidence::Confirmed);
    }
}
