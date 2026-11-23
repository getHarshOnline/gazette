//! Private CLI transport only; the application owns rule validation and scanning.

pub(super) use gazette_sdk::build::disclosure_input::WorkspaceInput as Input;
use gazette_sdk::build::{self, disclosure_input::ScanLimits};

#[derive(clap::Args)]
#[group(id = "DisclosureProbeLimits")]
pub(crate) struct Args {
    /// Maximum bytes presented to one percent/data decoding layer.
    #[arg(long, default_value_t = ScanLimits::default().encoded_input_bytes)]
    max_probe_encoded_input_bytes: usize,
    /// Maximum bytes produced by one percent/data decoding layer.
    #[arg(long, default_value_t = ScanLimits::default().decoded_bytes)]
    max_probe_decoded_bytes: usize,
    /// Maximum nested encoding layers; the scanner also enforces its hard ceiling.
    #[arg(long, default_value_t = ScanLimits::default().encoded_depth)]
    max_probe_encoded_depth: usize,
    /// Maximum encoded values inspected across the complete output tree.
    #[arg(long, default_value_t = ScanLimits::default().encoded_values)]
    max_probe_encoded_values: usize,
    /// Maximum aggregate decoded bytes across the complete output tree.
    #[arg(long, default_value_t = ScanLimits::default().encoded_bytes)]
    max_probe_encoded_bytes: usize,
    /// Shared literal-comparison work ceiling for raw and decoded channels.
    #[arg(long, default_value_t = ScanLimits::default().work_bytes)]
    max_probe_work_bytes: u64,
    /// Maximum tokenizer callbacks per HTML input, including embedded Markdown HTML.
    #[arg(long, default_value_t = ScanLimits::default().html_tokens)]
    max_probe_html_tokens: usize,
}

impl Args {
    pub(crate) fn limits(&self) -> build::DisclosureProbeLimits {
        ScanLimits {
            encoded_input_bytes: self.max_probe_encoded_input_bytes,
            decoded_bytes: self.max_probe_decoded_bytes,
            encoded_depth: self.max_probe_encoded_depth,
            encoded_values: self.max_probe_encoded_values,
            encoded_bytes: self.max_probe_encoded_bytes,
            work_bytes: self.max_probe_work_bytes,
            html_tokens: self.max_probe_html_tokens,
        }
        .native()
    }
}
