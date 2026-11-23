//! Provider-neutral external fact acquisition remains separate from deterministic compilation.

use std::cell::Cell;

use gazette_application::facts::{self, Error, Limits};
use gazette_compiler::{toml, workspace};
use gazette_core::publication::facts::{
    AcquisitionRequest, ProviderEvidence, ProviderSnapshot, SnapshotFact, Value,
};
use gazette_ports::facts::{ExternalFacts, Unavailable, UnavailableKind};

const LOCAL: &str = "[[principals]]\nid='owner:one'\nkind='person'\nname='Local name'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
const EXTERNAL: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[fact_sources]]\ntarget={kind='principal-name',principal='owner:one'}\nsource={kind='external',binding='input:external'}\n";

fn compile(source: &str) -> workspace::Compilation {
    workspace::compile(source, toml::Limits::default()).expect("valid fixture")
}

struct PanicProvider;

impl ExternalFacts for PanicProvider {
    fn acquire(&self, _: &AcquisitionRequest) -> Result<ProviderSnapshot, Unavailable> {
        panic!("fully local workspaces must not invoke an external provider")
    }
}

#[test]
fn fully_local_workspace_never_invokes_external_acquisition() {
    let compilation = compile(LOCAL);
    assert!(facts::request(&compilation.workspace).is_none());
    assert_eq!(
        facts::acquire(&PanicProvider, &compilation.workspace, Limits::default())
            .expect("local operation"),
        None
    );
}

struct CompleteProvider {
    calls: Cell<usize>,
}

impl ExternalFacts for CompleteProvider {
    fn acquire(&self, request: &AcquisitionRequest) -> Result<ProviderSnapshot, Unavailable> {
        self.calls.set(self.calls.get() + 1);
        let requested = request.facts()[0].clone();
        Ok(ProviderSnapshot::new(vec![SnapshotFact {
            requested,
            value: Value::PrincipalName {
                name: "Acquired name".into(),
            },
            evidence: ProviderEvidence {
                source: "record:owner-one".into(),
                revision: "revision:42".into(),
            },
        }]))
    }
}

#[test]
fn exact_provider_snapshot_is_frozen_but_not_silently_applied_to_compilation() {
    let compilation = compile(EXTERNAL);
    let provider = CompleteProvider {
        calls: Cell::new(0),
    };
    let capture = facts::acquire(&provider, &compilation.workspace, Limits::default())
        .expect("valid snapshot")
        .expect("external capture");
    assert_eq!(provider.calls.get(), 1);
    assert_eq!(capture.request().facts().len(), 1);
    assert_eq!(capture.snapshot().facts().len(), 1);
    assert_eq!(
        capture.snapshot().facts()[0].evidence.revision,
        "revision:42"
    );
    assert!(
        compilation
            .workspace
            .principals()
            .next()
            .expect("principal")
            .name
            .is_none(),
        "capture is not an implicit compiler overlay or local fallback"
    );
    assert!(compilation.workspace.require_resolved_facts().is_err());
}

struct RefusingProvider(UnavailableKind);

impl ExternalFacts for RefusingProvider {
    fn acquire(&self, request: &AcquisitionRequest) -> Result<ProviderSnapshot, Unavailable> {
        Err(Unavailable {
            requested: request.facts()[0].clone(),
            kind: self.0,
        })
    }
}

#[test]
fn unavailable_outcomes_are_exact_and_never_fall_back_to_local_literals() {
    let compilation = compile(EXTERNAL);
    for (kind, code) in [
        (
            UnavailableKind::Unconfigured,
            "publication_facts_unconfigured",
        ),
        (
            UnavailableKind::Unsupported,
            "publication_facts_unsupported",
        ),
        (UnavailableKind::Missing, "publication_facts_missing"),
        (UnavailableKind::Stale, "publication_facts_stale"),
        (UnavailableKind::Revoked, "publication_facts_revoked"),
        (
            UnavailableKind::Unauthorized,
            "publication_facts_unauthorized",
        ),
    ] {
        let error = facts::acquire(
            &RefusingProvider(kind),
            &compilation.workspace,
            Limits::default(),
        )
        .expect_err("explicit refusal");
        assert_eq!(error.code(), code);
        assert!(matches!(
            error,
            Error::Unconfigured(_)
                | Error::Unsupported(_)
                | Error::Missing(_)
                | Error::Stale(_)
                | Error::Revoked(_)
                | Error::Unauthorized(_)
        ));
        assert!(
            compilation
                .workspace
                .principals()
                .next()
                .expect("principal")
                .name
                .is_none()
        );
    }
}

struct PartialProvider;

impl ExternalFacts for PartialProvider {
    fn acquire(&self, _: &AcquisitionRequest) -> Result<ProviderSnapshot, Unavailable> {
        Ok(ProviderSnapshot::new(Vec::new()))
    }
}

#[test]
fn incomplete_snapshot_is_rejected_without_a_frozen_capture() {
    let compilation = compile(EXTERNAL);
    assert_eq!(
        facts::acquire(&PartialProvider, &compilation.workspace, Limits::default())
            .expect_err("partial snapshot"),
        Error::SnapshotMembership
    );
}
