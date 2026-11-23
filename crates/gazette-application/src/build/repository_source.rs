//! Build-time repository-source provenance shared by application evidence producers.

use gazette_core::source::Sha256Digest;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/repository_source_provenance.rs"));

/// Observed source inputs embedded when this application crate was built.
/// This includes declared native adapter/SDK/interface sources, but identifies neither
/// the embedding executable nor the selected compiled features or provider/toolchain binaries.
/// Concurrent source mutation and build-cache trust require independent qualification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RepositorySourceProvenance {
    profile: &'static str,
    sha256: Sha256Digest,
    files: u64,
    bytes: u64,
}

/// Portable identity of the running Gazette Engine build selected by qualification.
///
/// This coordinate deliberately excludes host paths, clocks, mutable remotes and
/// repository compatibility aliases. The compilation target is explicit because
/// it materially distinguishes portable release artifacts. Repository-source
/// provenance separately binds the exact selected source closure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBuildProfile {
    /// Gazette Engine package version embedded by Cargo.
    pub engine_version: String,
    /// Cargo build profile used for this application crate, including custom profiles.
    pub build_profile: String,
    /// Rust compilation target triple selected by Cargo.
    pub compilation_target: String,
}

impl RuntimeBuildProfile {
    /// Check bounded portable coordinate grammar without comparing an expected value.
    pub(crate) fn is_valid(&self) -> bool {
        [
            &self.engine_version,
            &self.build_profile,
            &self.compilation_target,
        ]
        .into_iter()
        .all(|value| {
            !value.is_empty()
                && value.len() <= 256
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'/' | b'\\'))
        })
    }
}

/// Return embedded provenance without reading the current checkout, Git, host or clock.
/// Relocating a built application cannot change this observation.
pub fn repository_source_provenance() -> RepositorySourceProvenance {
    RepositorySourceProvenance {
        profile: PROFILE,
        sha256: DIGEST.into(),
        files: FILES,
        bytes: BYTES,
    }
}

/// Return the running Gazette Engine/build-profile coordinate embedded at build time.
/// Relocation and runtime checkout state cannot change this coordinate.
pub fn runtime_build_profile() -> RuntimeBuildProfile {
    RuntimeBuildProfile {
        engine_version: ENGINE_VERSION.into(),
        build_profile: BUILD_PROFILE.into(),
        compilation_target: COMPILATION_TARGET.into(),
    }
}

/// Return one truthful human-readable coordinate for the running Gazette binary.
///
/// The coordinate identifies version, selected repository source, Cargo profile
/// and compilation target. It deliberately does not claim byte identity for the
/// executable, toolchain attestation or current-checkout equality.
pub fn runtime_version_coordinate() -> String {
    let source = repository_source_provenance();
    let runtime = runtime_build_profile();
    format!(
        "{} (source {}; profile {}; target {})",
        runtime.engine_version, source.sha256, runtime.build_profile, runtime.compilation_target
    )
}
