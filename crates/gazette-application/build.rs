//! Embed manifest-selected repository source provenance in the application.

#[path = "build_support/repository_source.rs"]
mod repository_source;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").ok_or("missing Cargo manifest root")?,
    );
    let root = manifest
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("missing Gazette source workspace")?;
    let snapshot = repository_source::snapshot(root)?;
    let engine_version = std::env::var("CARGO_PKG_VERSION")?;
    let build_profile = std::env::var("PROFILE")?;
    let compilation_target = std::env::var("TARGET")?;
    repository_source::validate_runtime_coordinate(&engine_version)?;
    repository_source::validate_runtime_coordinate(&build_profile)?;
    repository_source::validate_runtime_coordinate(&compilation_target)?;
    for path in &snapshot.watched {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    let output = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").ok_or("missing Cargo output directory")?,
    )
    .join("repository_source_provenance.rs");
    std::fs::write(
        output,
        format!(
            "const PROFILE: &str = {:?};\nconst DIGEST: [u8; 32] = {:?};\nconst FILES: u64 = {};\nconst BYTES: u64 = {};\nconst ENGINE_VERSION: &str = {:?};\nconst BUILD_PROFILE: &str = {:?};\nconst COMPILATION_TARGET: &str = {:?};\n",
            repository_source::PROFILE,
            snapshot.digest,
            snapshot.files,
            snapshot.bytes,
            engine_version,
            build_profile,
            compilation_target,
        ),
    )?;
    Ok(())
}
