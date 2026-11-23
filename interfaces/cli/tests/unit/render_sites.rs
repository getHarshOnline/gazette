//! One bound mapping feeds local rendering and both preview entry points.

use super::*;
use clap::Parser;

#[derive(Parser)]
struct Invocation {
    #[command(flatten)]
    render: Args,
}

#[test]
fn source_entry_override_preserves_other_native_bounds() {
    for (arguments, expected) in [
        (vec!["gazette", "--local"], 10_000),
        (
            vec!["gazette", "--local", "--max-total-source-entries", "16384"],
            16_384,
        ),
    ] {
        let (_, _, _, acquisition) = Invocation::try_parse_from(arguments)
            .unwrap()
            .render
            .limits();
        let mut expected_limits = build::PublicationSetAcquisitionLimits::default();
        expected_limits.sources.entries = expected;
        assert_eq!(
            serde_json::to_value(acquisition).unwrap(),
            serde_json::to_value(expected_limits).unwrap()
        );
    }
}

#[test]
fn resource_file_override_preserves_aggregate_and_decoder_bounds() {
    for (arguments, expected) in [
        (vec!["gazette", "--local"], 256 << 10),
        (
            vec!["gazette", "--local", "--max-resource-file-bytes", "4194304"],
            4 << 20,
        ),
    ] {
        let (_, limits, _, _) = Invocation::try_parse_from(arguments)
            .unwrap()
            .render
            .limits();
        let mut expected_limits = gazette_sdk::resource::Limits::default();
        expected_limits.inputs.file_bytes = expected;
        assert_eq!(
            serde_json::to_value(limits.publication.output.resources).unwrap(),
            serde_json::to_value(expected_limits).unwrap()
        );
    }
}
