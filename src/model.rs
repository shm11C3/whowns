use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    Confirmed,
    Probable,
    Unknown,
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

/// Stable machine-facing identity of an owner.
///
/// Ownership resolution and action guides branch on this value only. Display
/// text is derived from it, never the other way around, so renaming an owner
/// cannot change ownership-chain behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerId {
    Nix,
    Homebrew,
    MacPorts,
    Nvm,
    Fnm,
    Volta,
    Mise,
    Asdf,
    Pyenv,
    Rbenv,
    Sdkman,
    Uv,
    Rustup,
    RustupInstaller,
    CargoInstall,
    PnpmHome,
    DenoInstaller,
    BunInstaller,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    MacosInstaller,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PythonOrgInstaller,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Dpkg,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Rpm,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Pacman,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Apk,
    OperatingSystem,
    UnconfirmedOwner,
    UnconfirmedSource,
}

impl OwnerId {
    /// Every identity the tool can report. Platform-specific owners stay in the
    /// list so the identity set does not vary by build target.
    #[cfg(test)]
    pub const ALL: [Self; 27] = [
        Self::Nix,
        Self::Homebrew,
        Self::MacPorts,
        Self::Nvm,
        Self::Fnm,
        Self::Volta,
        Self::Mise,
        Self::Asdf,
        Self::Pyenv,
        Self::Rbenv,
        Self::Sdkman,
        Self::Uv,
        Self::Rustup,
        Self::RustupInstaller,
        Self::CargoInstall,
        Self::PnpmHome,
        Self::DenoInstaller,
        Self::BunInstaller,
        Self::MacosInstaller,
        Self::PythonOrgInstaller,
        Self::Dpkg,
        Self::Rpm,
        Self::Pacman,
        Self::Apk,
        Self::OperatingSystem,
        Self::UnconfirmedOwner,
        Self::UnconfirmedSource,
    ];

    /// Stable snake_case identifier. Part of the machine-readable contract:
    /// changing one of these values is a breaking change.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nix => "nix",
            Self::Homebrew => "homebrew",
            Self::MacPorts => "macports",
            Self::Nvm => "nvm",
            Self::Fnm => "fnm",
            Self::Volta => "volta",
            Self::Mise => "mise",
            Self::Asdf => "asdf",
            Self::Pyenv => "pyenv",
            Self::Rbenv => "rbenv",
            Self::Sdkman => "sdkman",
            Self::Uv => "uv",
            Self::Rustup => "rustup",
            Self::RustupInstaller => "rustup_installer",
            Self::CargoInstall => "cargo_install",
            Self::PnpmHome => "pnpm_home",
            Self::DenoInstaller => "deno_installer",
            Self::BunInstaller => "bun_installer",
            Self::MacosInstaller => "macos_installer",
            Self::PythonOrgInstaller => "python_org_installer",
            Self::Dpkg => "dpkg",
            Self::Rpm => "rpm",
            Self::Pacman => "pacman",
            Self::Apk => "apk",
            Self::OperatingSystem => "operating_system",
            Self::UnconfirmedOwner => "unconfirmed_owner",
            Self::UnconfirmedSource => "unconfirmed_source",
        }
    }

    /// Human-readable presentation text. Free to change without affecting
    /// ownership resolution.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Nix => "Nix",
            Self::Homebrew => "Homebrew",
            Self::MacPorts => "MacPorts",
            Self::Nvm => "nvm",
            Self::Fnm => "fnm",
            Self::Volta => "Volta",
            Self::Mise => "mise",
            Self::Asdf => "asdf",
            Self::Pyenv => "pyenv",
            Self::Rbenv => "rbenv",
            Self::Sdkman => "SDKMAN!",
            Self::Uv => "uv",
            Self::Rustup => "rustup",
            Self::RustupInstaller => "rustup installer",
            Self::CargoInstall => "cargo install",
            Self::PnpmHome => "pnpm home",
            Self::DenoInstaller => "Deno installer",
            Self::BunInstaller => "Bun installer",
            Self::MacosInstaller => "macOS Installer (.pkg)",
            Self::PythonOrgInstaller => "python.org macOS installer",
            Self::Dpkg => "dpkg",
            Self::Rpm => "RPM",
            Self::Pacman => "pacman",
            Self::Apk => "apk",
            Self::OperatingSystem => "operating system",
            Self::UnconfirmedOwner => "unconfirmed owner",
            Self::UnconfirmedSource => "unconfirmed source",
        }
    }

    /// How the owner installs software. Derived from identity so that an owner
    /// cannot be classified inconsistently between detection sites.
    pub fn kind(self) -> OwnerKind {
        match self {
            Self::Nix | Self::Homebrew | Self::MacPorts => OwnerKind::PackageManager,
            Self::Dpkg | Self::Rpm | Self::Pacman | Self::Apk => OwnerKind::PackageManager,
            Self::Nvm
            | Self::Fnm
            | Self::Volta
            | Self::Mise
            | Self::Asdf
            | Self::Pyenv
            | Self::Rbenv
            | Self::Sdkman
            | Self::Uv
            | Self::Rustup => OwnerKind::VersionManager,
            Self::RustupInstaller
            | Self::CargoInstall
            | Self::PnpmHome
            | Self::DenoInstaller
            | Self::BunInstaller => OwnerKind::ToolInstaller,
            Self::MacosInstaller | Self::PythonOrgInstaller => OwnerKind::Installer,
            Self::OperatingSystem => OwnerKind::OperatingSystem,
            Self::UnconfirmedOwner | Self::UnconfirmedSource => OwnerKind::Unknown,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Evidence {
    pub source: String,
    pub detail: String,
}

impl Evidence {
    pub fn new(source: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            detail: detail.into(),
        }
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
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub actions: ActionGuide,
}

impl OwnershipNode {
    pub fn display_name(&self) -> &'static str {
        self.id.display_name()
    }

    pub fn kind(&self) -> OwnerKind {
        self.id.kind()
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
        let node = OwnershipNode {
            id: OwnerId::Rustup,
            package: None,
            version: None,
            confidence: Confidence::Confirmed,
            evidence: vec![],
            actions: ActionGuide::default(),
        };
        assert_eq!(node.kind(), OwnerKind::VersionManager);
        assert_eq!(node.display_name(), "rustup");
        assert_eq!(OwnerId::RustupInstaller.kind(), OwnerKind::ToolInstaller);
    }
}
