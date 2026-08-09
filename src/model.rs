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
    // Installer receipts are currently detected only on macOS.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
    pub name: String,
    pub kind: OwnerKind,
    pub package: Option<String>,
    pub version: Option<String>,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub actions: ActionGuide,
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
