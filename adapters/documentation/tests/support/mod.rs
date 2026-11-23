//! Disposable native job construction shared by adapter-boundary fixtures.

use gazette_adapter_fs::FileSystem;
use gazette_core::{
    documentation::generator::{Recipe, Request},
    process::{ExecutablePin, Options},
    source::{CapturedSource, Content, InventoryBuilder, Limits, Sha256Digest},
    stage::{Location, RetainedStage},
};
use gazette_ports::stage::StageWriter as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Duration,
};
use tempfile::TempDir;

pub(super) struct Job {
    pub(super) input: RetainedStage,
    pub(super) output: RetainedStage,
    pub(super) recipe: Recipe,
    _input_parent: TempDir,
    _output_parent: TempDir,
}

pub(super) fn pin(program: &Path) -> ExecutablePin {
    let bytes = fs::read(program).expect("explicit fixture executable");
    ExecutablePin {
        bytes: bytes.len() as u64,
        sha256: Sha256Digest::of(&bytes),
    }
}

impl Job {
    pub(super) fn new(files: &[(&str, &[u8])], recipe: Recipe) -> Self {
        let input_parent = tempfile::tempdir().expect("owned input parent");
        let output_parent = tempfile::tempdir().expect("independent output parent");
        let mut inventory = InventoryBuilder::new(Limits::default());
        let mut contents = BTreeMap::new();
        let mut directories = BTreeSet::new();
        for &(name, bytes) in files {
            let path: gazette_core::source::EntryPath = name.parse().expect("fixture filename");
            for (end, _) in name.match_indices('/') {
                let directory: gazette_core::source::EntryPath =
                    name[..end].parse().expect("fixture directory");
                if directories.insert(directory.clone()) {
                    inventory
                        .insert(directory, Content::Directory)
                        .expect("fixture parent");
                }
            }
            inventory
                .insert(
                    path.clone(),
                    Content::File {
                        bytes: bytes.len() as u64,
                        sha256: Sha256Digest::of(bytes),
                    },
                )
                .expect("fixture file");
            contents.insert(path, bytes.to_vec());
        }
        let captured =
            CapturedSource::from_parts(inventory.finish(), contents).expect("fixture capture");
        let input = FileSystem
            .stage(
                Location::Fresh(input_parent.path()),
                &[],
                captured.as_input(),
                Limits::default(),
            )
            .expect("real input stage")
            .retain()
            .expect("retained input");
        let empty = CapturedSource::from_parts(
            InventoryBuilder::new(Limits::default()).finish(),
            BTreeMap::new(),
        )
        .expect("empty output capture");
        let output = FileSystem
            .stage(
                Location::Fresh(output_parent.path()),
                &[input.path().to_path_buf()],
                empty.as_input(),
                Limits::default(),
            )
            .expect("separate output stage")
            .retain()
            .expect("retained output");
        Self {
            input,
            output,
            recipe,
            _input_parent: input_parent,
            _output_parent: output_parent,
        }
    }

    pub(super) fn request(&self) -> Request<'_> {
        Request {
            recipe: &self.recipe,
            input: &self.input,
            output: &self.output,
            input_limits: Limits::default(),
            output_limits: Limits::default(),
            execution: Options {
                allow_execute: true,
                timeout: Duration::from_secs(30),
                ..Options::default()
            },
            max_capture_bytes: 1 << 20,
        }
    }
}
