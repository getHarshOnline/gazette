//! Bounded, immutable configured-output history. Stored native facts are not permits.

use crate::{Sqlite, native_error, registry, with_source};
use gazette_core::{
    destination::{OutputAdministrationBinding, OutputBinding, OutputIdentity},
    publication::Key,
    source::{Content, EntryPath, Inventory, InventoryBuilder, Limits, Sha256Digest, TreeIdentity},
    store::{
        Error, JournalHeader, MaterializationCompletion, MaterializationFrontier,
        MaterializationInput, MaterializationInspection, MaterializationIntent,
        MaterializationRecord, ReadOptions,
    },
};
use gazette_ports::store::MaterializationReader;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    device: u64,
    inode: u64,
}
impl Identity {
    fn native(self) -> OutputIdentity {
        OutputIdentity::new(self.device, self.inode)
    }
}
#[derive(Deserialize)]
enum Administration {
    Directory(Identity),
    File(Identity),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    canonical_root: PathBuf,
    root: Identity,
    administration: Option<Administration>,
}
impl Binding {
    fn native(self) -> Result<OutputBinding, Error> {
        root(&self.canonical_root)?;
        OutputBinding::new(
            self.canonical_root,
            self.root.native(),
            self.administration.map(|admin| match admin {
                Administration::Directory(identity) => {
                    OutputAdministrationBinding::Directory(identity.native())
                }
                Administration::File(identity) => {
                    OutputAdministrationBinding::File(identity.native())
                }
            }),
        )
        .map_err(|_| Error::MaterializationVerification)
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Directory {
        path: EntryPath,
    },
    File {
        path: EntryPath,
        bytes: u64,
        sha256: Sha256Digest,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    entries: Vec<Entry>,
    total_bytes: u64,
    identity: TreeIdentity,
}
impl Manifest {
    fn native(self, options: ReadOptions) -> Result<Inventory, Error> {
        // Entry metadata is bounded by the already admitted encoded payload. File
        // byte counts describe output artifacts, not retained payload allocations.
        let mut builder = InventoryBuilder::new(Limits {
            entries: usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?,
            depth: usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?,
            file_bytes: u64::MAX,
            total_bytes: u64::MAX,
        });
        for entry in self.entries {
            let (path, content) = match entry {
                Entry::Directory { path } => (path, Content::Directory),
                Entry::File {
                    path,
                    bytes,
                    sha256,
                } => (path, Content::File { bytes, sha256 }),
            };
            builder
                .insert(path, content)
                .map_err(|_| Error::MaterializationVerification)?;
        }
        let inventory = builder.finish();
        if inventory.total_bytes() != self.total_bytes || inventory.identity() != self.identity {
            return Err(Error::MaterializationVerification);
        }
        Ok(inventory)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    id: Sha256Digest,
    site: Key,
    binding: Binding,
    previous: Option<Sha256Digest>,
    observed: Manifest,
    expected: Manifest,
    generated: Manifest,
    protected: Vec<PathBuf>,
    evidence: String,
    evidence_sha256: Sha256Digest,
    created: JournalHeader,
}
impl Intent {
    fn native(self, options: ReadOptions) -> Result<MaterializationIntent, Error> {
        let intent = MaterializationIntent {
            id: self.id,
            site: self.site,
            binding: self.binding.native()?,
            previous: self.previous,
            observed: self.observed.native(options)?,
            expected: self.expected.native(options)?,
            generated: self.generated.native(options)?,
            protected: self.protected,
            evidence: self.evidence,
            evidence_sha256: self.evidence_sha256,
            created: self.created,
        };
        validate(intent.as_input(), options)?;
        Ok(intent)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    intent_id: Sha256Digest,
    output: TreeIdentity,
    entries: u64,
    bytes: u64,
    recorded: JournalHeader,
}
impl Completion {
    fn native(self) -> MaterializationCompletion {
        MaterializationCompletion {
            intent_id: self.intent_id,
            output: self.output,
            entries: self.entries,
            bytes: self.bytes,
            recorded: self.recorded,
        }
    }
}

pub(super) fn root(path: &Path) -> Result<&str, Error> {
    let value = path.to_str().ok_or(Error::MaterializationVerification)?;
    let normalized: PathBuf = path.components().collect();
    if !path.is_absolute()
        || normalized.to_str() != Some(value)
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        || value.chars().any(char::is_control)
    {
        return Err(Error::MaterializationVerification);
    }
    Ok(value)
}

pub(super) fn encode(value: &impl Serialize, options: ReadOptions) -> Result<String, Error> {
    options.validate()?;
    let maximum = usize::try_from(
        u64::try_from(options.value_bytes)
            .map_err(|_| Error::InvalidLimits)?
            .min(options.storage_bytes),
    )
    .map_err(|_| Error::InvalidLimits)?;
    let mut output = gazette_core::render::LimitedWriter::new(Vec::new(), maximum);
    let result = serde_json::to_writer(&mut output, value);
    if output.exceeded() {
        return Err(Error::StorageLimit);
    }
    result.map_err(|_| Error::MaterializationVerification)?;
    String::from_utf8(output.into_inner()).map_err(|_| Error::MaterializationVerification)
}

#[derive(Serialize)]
struct Payload<'a> {
    id: Sha256Digest,
    site: &'a Key,
    binding: &'a OutputBinding,
    previous: Option<Sha256Digest>,
    observed: &'a Inventory,
    expected: &'a Inventory,
    generated: &'a Inventory,
    protected: &'a [PathBuf],
    evidence: &'a str,
    evidence_sha256: Sha256Digest,
    created: JournalHeader,
}
pub(super) fn payload(
    input: MaterializationInput<'_>,
    created: JournalHeader,
    options: ReadOptions,
) -> Result<String, Error> {
    encode(
        &Payload {
            id: input.id,
            site: input.site,
            binding: input.binding,
            previous: input.previous,
            observed: input.observed,
            expected: input.expected,
            generated: input.generated,
            protected: input.protected,
            evidence: input.evidence,
            evidence_sha256: input.evidence_sha256,
            created,
        },
        options,
    )
}
pub(super) fn validate(input: MaterializationInput<'_>, options: ReadOptions) -> Result<(), Error> {
    payload(
        input,
        JournalHeader {
            schema_version: crate::schema::VERSION,
            journal_id: [0; 32],
            journal_epoch: 1,
            writer_fence: 1,
        },
        options,
    )?;
    root(input.binding.canonical_root())?;
    for path in input.protected {
        root(path)?;
    }
    if input.previous == Some(input.id)
        || Sha256Digest::of(input.evidence.as_bytes()) != input.evidence_sha256
    {
        return Err(Error::MaterializationVerification);
    }
    // Bound the complete borrowed payload before parsing or retaining evidence.
    gazette_core::json::parse(input.evidence.as_bytes())
        .map_err(|_| Error::MaterializationVerification)?;
    for generated in input.generated.entries() {
        let actual = input
            .expected
            .entries()
            .binary_search_by(|entry| entry.path.cmp(&generated.path))
            .ok()
            .and_then(|index| input.expected.entries().get(index));
        if actual != Some(generated) {
            return Err(Error::MaterializationVerification);
        }
    }
    Ok(())
}
fn historical(header: JournalHeader, current: JournalHeader) -> Result<(), Error> {
    if header.schema_version != current.schema_version
        || header.journal_id != current.journal_id
        || header.journal_epoch != current.journal_epoch
        || header.writer_fence == 0
        || header.writer_fence > current.writer_fence
    {
        return Err(Error::MaterializationVerification);
    }
    Ok(())
}
fn debit(value: &str, remaining: &mut u64, options: ReadOptions) -> Result<(), Error> {
    let length = u64::try_from(value.len()).map_err(|_| Error::StorageLimit)?;
    if length > u64::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)? {
        return Err(Error::StorageLimit);
    }
    *remaining = remaining
        .checked_sub(length.checked_add(128).ok_or(Error::StorageLimit)?)
        .ok_or(Error::StorageLimit)?;
    Ok(())
}

fn read_record(
    connection: &Connection,
    id: Sha256Digest,
    current: JournalHeader,
    options: ReadOptions,
    remaining: &mut u64,
) -> Result<Option<MaterializationRecord>, Error> {
    let mut statement = connection.prepare("SELECT root,payload,payload_sha256 FROM gazette_materialization_intents WHERE id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([id.as_bytes()]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let text = row
        .get_ref(1)
        .map_err(native_error)?
        .as_str()
        .map_err(|_| Error::MaterializationVerification)?;
    debit(text, remaining, options)?;
    let digest: [u8; 32] = row.get(2).map_err(|_| Error::MaterializationVerification)?;
    if Sha256Digest::of(text.as_bytes()).as_bytes() != &digest {
        return Err(Error::MaterializationVerification);
    }
    let parsed = gazette_core::json::parse(text.as_bytes())
        .map_err(|_| Error::MaterializationVerification)?;
    let intent = serde_json::from_value::<Intent>(parsed)
        .map_err(|_| Error::MaterializationVerification)?
        .native(options)?;
    if intent.id != id
        || row
            .get_ref(0)
            .map_err(native_error)?
            .as_str()
            .map_err(|_| Error::MaterializationVerification)?
            != root(intent.binding.canonical_root())?
        || encode(&intent, options)? != text
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::MaterializationVerification);
    }
    historical(intent.created, current)?;
    let mut statement = connection.prepare("SELECT payload,payload_sha256 FROM gazette_materialization_completions WHERE intent_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([id.as_bytes()]).map_err(native_error)?;
    let completion = if let Some(row) = rows.next().map_err(native_error)? {
        let text = row
            .get_ref(0)
            .map_err(native_error)?
            .as_str()
            .map_err(|_| Error::MaterializationVerification)?;
        debit(text, remaining, options)?;
        let digest: [u8; 32] = row.get(1).map_err(|_| Error::MaterializationVerification)?;
        if Sha256Digest::of(text.as_bytes()).as_bytes() != &digest {
            return Err(Error::MaterializationVerification);
        }
        let parsed = gazette_core::json::parse(text.as_bytes())
            .map_err(|_| Error::MaterializationVerification)?;
        let completion = serde_json::from_value::<Completion>(parsed)
            .map_err(|_| Error::MaterializationVerification)?
            .native();
        historical(completion.recorded, current)?;
        completion.validate(&intent)?;
        if encode(&completion, options)? != text {
            return Err(Error::MaterializationVerification);
        }
        Some(completion)
    } else {
        None
    };
    if rows.next().map_err(native_error)?.is_some() {
        return Err(Error::MaterializationVerification);
    }
    Ok(Some(MaterializationRecord { intent, completion }))
}

pub(super) fn read(
    connection: &Connection,
    id: Sha256Digest,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<MaterializationRecord>, Error> {
    let mut remaining = options.storage_bytes;
    read_record(connection, id, current, options, &mut remaining)
}

pub(super) fn frontier(
    connection: &Connection,
    selected: &Path,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<MaterializationFrontier, Error> {
    let mut remaining = options.storage_bytes;
    frontier_budget(connection, selected, current, options, &mut remaining)
}
fn frontier_budget(
    connection: &Connection,
    selected: &Path,
    current: JournalHeader,
    options: ReadOptions,
    remaining: &mut u64,
) -> Result<MaterializationFrontier, Error> {
    let selected_text = root(selected)?;
    let slot = connection
        .query_row(
            "SELECT head,active FROM gazette_materialization_frontiers WHERE root=?1",
            [selected_text],
            |row| {
                let head: Option<[u8; 32]> = row.get(0)?;
                let active: Option<[u8; 32]> = row.get(1)?;
                Ok((head, active))
            },
        )
        .optional()
        .map_err(native_error)?;
    let mut statement = connection
        .prepare("SELECT id FROM gazette_materialization_intents WHERE root=?1 ORDER BY id")
        .map_err(native_error)?;
    let mut rows = statement.query([selected_text]).map_err(native_error)?;
    let mut records = BTreeMap::new();
    while let Some(row) = rows.next().map_err(native_error)? {
        let id = Sha256Digest::from(
            row.get::<_, [u8; 32]>(0)
                .map_err(|_| Error::MaterializationVerification)?,
        );
        let record = read_record(connection, id, current, options, remaining)?
            .ok_or(Error::MaterializationVerification)?;
        records.insert(id, record);
    }
    let Some((head, active)) = slot else {
        if !records.is_empty() {
            return Err(Error::MaterializationVerification);
        }
        return Ok(MaterializationFrontier {
            root: selected.into(),
            head: None,
            active: None,
        });
    };
    let head = head.map(Sha256Digest::from);
    let active = active.map(Sha256Digest::from);
    if head.is_none() && active.is_none() {
        return Err(Error::MaterializationVerification);
    }
    let mut successors = BTreeMap::new();
    let mut starting = None;
    for (id, record) in &records {
        if record.intent.binding.canonical_root() != selected {
            return Err(Error::MaterializationVerification);
        }
        if let Some(previous) = record.intent.previous {
            let predecessor = records
                .get(&previous)
                .ok_or(Error::MaterializationVerification)?;
            let completed = predecessor
                .completion
                .as_ref()
                .ok_or(Error::MaterializationVerification)?;
            if predecessor.intent.site != record.intent.site
                || predecessor.intent.binding != record.intent.binding
                || record.intent.created.writer_fence < completed.recorded.writer_fence
                || successors.insert(previous, *id).is_some()
            {
                return Err(Error::MaterializationVerification);
            }
        } else if starting.replace(*id).is_some() {
            return Err(Error::MaterializationVerification);
        }
        if record.completion.is_none() && active != Some(*id) {
            return Err(Error::MaterializationVerification);
        }
    }
    let mut visited = BTreeSet::new();
    let mut cursor = starting.ok_or(Error::MaterializationVerification)?;
    let mut last_complete = None;
    loop {
        if !visited.insert(cursor) {
            return Err(Error::MaterializationVerification);
        }
        let record = records
            .get(&cursor)
            .ok_or(Error::MaterializationVerification)?;
        if record.completion.is_some() {
            last_complete = Some(cursor);
        }
        if let Some(next) = successors.get(&cursor) {
            cursor = *next;
        } else {
            break;
        }
    }
    if visited.len() != records.len()
        || head != last_complete
        || active
            != records
                .get(&cursor)
                .filter(|record| record.completion.is_none())
                .map(|_| cursor)
    {
        return Err(Error::MaterializationVerification);
    }
    let head = head
        .map(|id| {
            records
                .remove(&id)
                .ok_or(Error::MaterializationVerification)
        })
        .transpose()?;
    let active = active
        .map(|id| {
            records
                .remove(&id)
                .ok_or(Error::MaterializationVerification)
        })
        .transpose()?;
    Ok(MaterializationFrontier {
        root: selected.into(),
        head,
        active,
    })
}

pub(super) fn verify_all(
    connection: &Connection,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<(), Error> {
    scan_all(connection, current, options, false)
}
fn scan_all(
    connection: &Connection,
    current: JournalHeader,
    options: ReadOptions,
    terminal: bool,
) -> Result<(), Error> {
    let mut remaining = options.storage_bytes;
    let mut statement = connection.prepare("SELECT root FROM gazette_materialization_frontiers UNION SELECT root FROM gazette_materialization_intents ORDER BY root").map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    while let Some(row) = rows.next().map_err(native_error)? {
        let path = row
            .get_ref(0)
            .map_err(native_error)?
            .as_str()
            .map_err(|_| Error::MaterializationVerification)?;
        debit(path, &mut remaining, options)?;
        let history = frontier_budget(
            connection,
            Path::new(path),
            current,
            options,
            &mut remaining,
        )?;
        if terminal && history.active.is_some() {
            return Err(Error::MaterializationPending);
        }
    }
    Ok(())
}
pub(super) fn require_terminal(connection: &Connection, options: ReadOptions) -> Result<(), Error> {
    let inspection = registry::inspect(connection, options)?;
    scan_all(connection, inspection.journal.header, options, true)
}

pub(super) fn insert_intent(
    connection: &Connection,
    input: MaterializationInput<'_>,
    created: JournalHeader,
    options: ReadOptions,
) -> Result<(), Error> {
    let text = payload(input, created, options)?;
    let selected = root(input.binding.canonical_root())?;
    connection.execute("INSERT INTO gazette_materialization_intents(id,root,payload,payload_sha256) VALUES(?1,?2,?3,?4)", params![input.id.as_bytes(), selected, text, Sha256Digest::of(text.as_bytes()).as_bytes()]).map_err(native_error)?;
    let changed = if let Some(previous) = input.previous {
        connection.execute("UPDATE gazette_materialization_frontiers SET active=?1 WHERE root=?2 AND head=?3 AND active IS NULL", params![input.id.as_bytes(), selected, previous.as_bytes()]).map_err(native_error)?
    } else {
        connection.execute("INSERT INTO gazette_materialization_frontiers(root,head,active) VALUES(?1,NULL,?2)", params![selected,input.id.as_bytes()]).map_err(native_error)?
    };
    if changed != 1 {
        return Err(Error::MaterializationConflict);
    }
    Ok(())
}
pub(super) fn insert_completion(
    connection: &Connection,
    intent: &MaterializationIntent,
    recorded: JournalHeader,
    options: ReadOptions,
) -> Result<(), Error> {
    let completion = MaterializationCompletion {
        intent_id: intent.id,
        output: intent.expected.identity(),
        entries: u64::try_from(intent.expected.entries().len()).map_err(|_| Error::StorageLimit)?,
        bytes: intent.expected.total_bytes(),
        recorded,
    };
    let text = encode(&completion, options)?;
    connection.execute("INSERT INTO gazette_materialization_completions(intent_id,payload,payload_sha256) VALUES(?1,?2,?3)", params![intent.id.as_bytes(), text,Sha256Digest::of(text.as_bytes()).as_bytes()]).map_err(native_error)?;
    let changed = connection.execute("UPDATE gazette_materialization_frontiers SET head=?1,active=NULL WHERE root=?2 AND active=?1 AND head IS ?3", params![intent.id.as_bytes(),root(intent.binding.canonical_root())?,intent.previous.map(|id| id.as_bytes().to_vec())]).map_err(native_error)?;
    if changed != 1 {
        return Err(Error::MaterializationConflict);
    }
    Ok(())
}

impl MaterializationReader for Sqlite {
    fn inspect_materialization(
        &self,
        path: &Path,
        selected: &Path,
        options: ReadOptions,
    ) -> Result<MaterializationInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            verify_all(connection, inspection.journal.header, options)?;
            let frontier = frontier(connection, selected, inspection.journal.header, options)?;
            Ok(MaterializationInspection {
                journal: inspection.journal,
                frontier,
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/materialization.rs"]
mod tests;
