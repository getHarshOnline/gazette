//! Configured branch and index setup without touching working files or detached release custody.

use super::{Git, Result, check_namespace, custody, history, setup};
use gazette_core::{
    destination::{OutputAdministrationBinding, OutputBinding, OutputIdentity},
    release::*,
    source::MetadataBudget,
};
use gazette_ports::release::OutputCheckout;
use git2::{
    Config, Index, ObjectFormat, Oid, ReferenceType, Repository, RepositoryInitOptions,
    RepositoryOpenFlags,
};
use std::{collections::BTreeSet, fs, path::Path};

impl OutputCheckout for Git {
    fn prepare_output_checkout(
        &self,
        request: OutputCheckoutRequest<'_>,
        options: OutputCheckoutOptions,
    ) -> std::result::Result<OutputCheckoutReport, RepositorySetupError> {
        let mut effect = RepositorySetupEffect::NotAttempted;
        (|| {
            let PublicationDestination::Git {remote,branch}=request.destination;
            validate_git_destination(remote,branch).map_err(|_| "configured output destination is invalid")?;
            match request.expected {
                OutputCheckoutState::Missing | OutputCheckoutState::Unborn if !options.allow_initialize => return Err("explicit output repository initialization permission is required"),
                OutputCheckoutState::Commit {..} if !options.allow_update => return Err("explicit output repository update permission is required"),
                _=>(),
            }
            if options.limits.storage_bytes==0 { return Err("output repository storage ceiling is zero"); }
            let root=setup::coordinate(request.root)?;
            setup::separate(&root,request.protected_roots)?;
            if Path::new(remote).is_absolute() { setup::separate(&root,&[remote.into()])?; }
            else if let Ok(url)=url::Url::parse(remote) && let Ok(path)=url.to_file_path() { setup::separate(&root,&[path])?; }
            let administrative=root.join(".git");
            let mut budget=MetadataBudget::new(options.limits.metadata_bytes);
            let root_held=match fs::symlink_metadata(&root) {
                Ok(_) => Some(setup::Directory::open(&root)?),
                Err(e) if e.kind()==std::io::ErrorKind::NotFound && matches!(request.expected,OutputCheckoutState::Missing)=>None,
                _=>return Err("configured output root is missing or unavailable"),
            };
            let configuration=configuration(remote,branch);
            budget.charge(configuration.len()).map_err(|_| "output configuration ceiling exceeded")?;
            let existing=match request.expected {
                OutputCheckoutState::Missing=>{setup::absent(&administrative)?;None},
                _=>Some(open(&root,&mut budget)?),
            };
            if let Some((repo,held))=&existing {
                custody::inspect_storage(&administrative,options.limits.storage_bytes,&mut budget)?;
                prestate(repo,request.expected,branch,true)?;
                match request.expected {
                    OutputCheckoutState::Unborn=>{
                        if setup::read(&administrative.join("config"),&mut budget)?!=configuration.as_bytes() {
                            validate_unborn_config(repo,remote)?;
                        }
                    },
                    OutputCheckoutState::Commit {oid}=>{
                        if setup::read(&administrative.join("config"),&mut budget)?!=configuration.as_bytes() { return Err("existing output configuration differs from the owned tracking profile"); }
                        history::verify(repo,oid,options.limits.history,&mut budget)?;
                        inspect_index(repo,Some(oid))?;
                    },
                    _=>(),
                }
                held.recheck()?;
            }
            let retained=match request.parent {
                Some(parent)=>Some(history::capture_for_setup(&parent.source,&root,&parent.commit,options.limits.history,options.limits.storage_bytes,&mut budget)?),
                None if matches!(request.expected,OutputCheckoutState::Commit {..})=>return Err("existing output branch cannot become unborn"),
                None=>None,
            };
            // An existing commit's ancestry must be proved from the selected complete source
            // before changing local objects, refs, index or configuration.
            if let (OutputCheckoutState::Commit {oid},Some(parent))=(request.expected,request.parent) {
                descendant(&parent.source,&parent.commit,oid,&mut budget)?;
            }
            effect=RepositorySetupEffect::LocalStateMayRemain;
            let root_held=match root_held {Some(held)=>held,None=>setup::Directory::create(&root)?};
            let (repo,administrative_held)=match existing {
                Some(value)=>value,
                None=>{
                    let held=setup::Directory::create(&administrative)?;
                    let mut initialization=RepositoryInitOptions::new();
                    initialization.bare(false).no_reinit(true).mkdir(false).mkpath(false).no_dotgit_dir(false)
                        .external_template(false).template_path(Path::new("")).initial_head(&format!("refs/heads/{branch}"))
                        .object_format(ObjectFormat::Sha1);
                    let repo=Repository::init_opts(&root,&initialization).map_err(|_| "output repository initialization failed")?;
                    (repo,held)
                },
            };
            root_held.recheck()?; administrative_held.recheck()?;
            fs::write(administrative.join("config"),configuration.as_bytes()).map_err(|_| "output tracking configuration write failed")?;
            let local=Config::open(&administrative.join("config")).map_err(|_| "output configuration cannot be bound")?;
            repo.set_config(&local).map_err(|_| "output configuration cannot be isolated")?;
            if let Some(retained)=retained {retained.write(&repo)?;}
            let reference=format!("refs/heads/{branch}");
            let tracking=format!("refs/remotes/origin/{branch}");
            let mut transaction=repo.transaction().map_err(|_| "output ref transaction unavailable")?;
            for name in ["HEAD",reference.as_str(),tracking.as_str()] { transaction.lock_ref(name).map_err(|_| "output refs are busy; preserve local state")?; }
            prestate(&repo,request.expected,branch,false)?;
            let parent=request.parent.map(|parent|&parent.commit);
            let mut index=repo.index().map_err(|_| "output index unavailable")?;
            if let Some(parent)=parent {
                let oid=Oid::from_str(parent.as_str()).map_err(|_| "output parent is invalid")?;
                let tree=repo.find_commit(oid).and_then(|c|c.tree()).map_err(|_| "output parent tree unavailable")?;
                index.read_tree(&tree).map_err(|_| "output index cannot load parent tree")?;
                for name in [&reference,&tracking] { transaction.set_target(name,oid,None,"Gazette configured output history").map_err(|_| "output ref transaction refused")?; }
            } else { index.clear().map_err(|_| "output index cannot remain empty")?; }
            index.write().map_err(|_| "output index write failed")?;
            transaction.set_symbolic_target("HEAD",&reference,None,"Gazette configured output branch").map_err(|_| "output HEAD transaction refused")?;
            transaction.commit().map_err(|_| "output ref transaction failed; preserve metadata")?;
            root_held.recheck()?; administrative_held.recheck()?;
            inspect(&repo,remote,branch,parent,&configuration,&administrative,&mut budget)?;
            custody::synchronize(&administrative,options.limits.storage_bytes,&mut budget)?;
            let (reopened,reopened_held)=open(&root,&mut budget)?;
            inspect(&reopened,remote,branch,parent,&configuration,&administrative,&mut budget)?;
            if let Some(parent)=parent {history::verify(&reopened,parent,options.limits.history,&mut budget)?;}
            root_held.synchronize()?; administrative_held.recheck()?; reopened_held.recheck()?;
            let binding=OutputBinding::new(root,OutputIdentity::new(root_held.identity.0,root_held.identity.1),Some(OutputAdministrationBinding::Directory(OutputIdentity::new(administrative_held.identity.0,administrative_held.identity.1))))
                .map_err(|_| "output native identity cannot be retained")?;
            Ok(OutputCheckoutReport {binding,destination:request.destination.clone(),parent:parent.cloned()})
        })().map_err(|reason|RepositorySetupError {reason,effect,cleanup:None})
    }
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn configuration(remote: &str, branch: &str) -> String {
    format!(
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = false\n[remote \"origin\"]\n\turl = {}\n\tfetch = {}\n[branch {}]\n\tremote = origin\n\tmerge = {}\n",
        quote(remote),
        quote(&format!(
            "+refs/heads/{branch}:refs/remotes/origin/{branch}"
        )),
        quote(branch),
        quote(&format!("refs/heads/{branch}"))
    )
}

fn open(root: &Path, budget: &mut MetadataBudget) -> Result<(Repository, setup::Directory)> {
    let administrative = root.join(".git");
    let held = setup::Directory::open(&administrative)?;
    check_namespace(&administrative, budget)?;
    let bytes = setup::read(&administrative.join("config"), budget)?;
    preflight_config(&bytes)?;
    // Reject active hooks even though metadata writes never invoke a hook.
    if let Ok(entries) = fs::read_dir(administrative.join("hooks")) {
        for entry in entries {
            let entry = entry.map_err(|_| "output hooks cannot be inspected")?;
            if !entry.file_name().to_string_lossy().ends_with(".sample") {
                return Err("active output hooks are unsupported");
            }
        }
    }
    let repo = Repository::open_ext(root, RepositoryOpenFlags::NO_SEARCH, &[] as &[&str])
        .map_err(|_| "independent output repository required")?;
    if repo.is_bare()
        || repo
            .workdir()
            .and_then(|p| fs::canonicalize(p).ok())
            .as_deref()
            != Some(root)
        || fs::canonicalize(repo.path()).ok().as_deref() != Some(administrative.as_path())
        || fs::canonicalize(repo.commondir()).ok().as_deref() != Some(administrative.as_path())
    {
        return Err("output repository is not independent");
    }
    let config = Config::open(&administrative.join("config"))
        .map_err(|_| "output local configuration unavailable")?;
    repo.set_config(&config)
        .map_err(|_| "output local configuration cannot be isolated")?;
    held.recheck()?;
    Ok((repo, held))
}

fn preflight_config(bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| "output configuration is not text")?;
    let mut section = "";
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            if line == "[core]" {
                section = "core";
            } else if line == "[remote \"origin\"]" {
                section = "remote";
            } else if line.starts_with("[branch \"") && line.ends_with("\"]") {
                section = "branch";
            } else {
                return Err("output configuration contains an unsupported section");
            }
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or("output configuration requires explicit values")?;
        let key = key.trim().to_ascii_lowercase();
        if value.trim_end().ends_with('\\') || !seen.insert((section, key.clone())) {
            return Err("output configuration contains repeated or continued values");
        }
        let allowed = match section {
            "core" => matches!(
                key.as_str(),
                "repositoryformatversion"
                    | "filemode"
                    | "bare"
                    | "logallrefupdates"
                    | "ignorecase"
                    | "precomposeunicode"
            ),
            "remote" => matches!(key.as_str(), "url" | "fetch"),
            "branch" => matches!(key.as_str(), "remote" | "merge"),
            _ => false,
        };
        if !allowed {
            return Err("output configuration contains an unsupported key");
        }
    }
    Ok(())
}

fn validate_unborn_config(repo: &Repository, remote: &str) -> Result<()> {
    let config = repo
        .config()
        .map_err(|_| "output configuration unavailable")?;
    if config.get_i64("core.repositoryformatversion").ok() != Some(0)
        || config.get_bool("core.bare").ok() != Some(false)
    {
        return Err("unborn output core configuration is unsupported");
    }
    if let Ok(selected) = config.get_string("remote.origin.url")
        && selected != remote
    {
        return Err("unborn output origin differs from the configured destination");
    }
    let mut entries = config
        .entries(None)
        .map_err(|_| "unborn output configuration unavailable")?;
    while let Some(entry) = entries.next() {
        if entry
            .map_err(|_| "unborn output configuration invalid")?
            .name()
            .map_err(|_| "unborn output configuration name is not text")?
            .starts_with("branch.")
        {
            return Err("unborn output already has branch tracking configuration");
        }
    }
    Ok(())
}

fn prestate(
    repo: &Repository,
    expected: &OutputCheckoutState,
    branch: &str,
    empty_objects: bool,
) -> Result<()> {
    let head = repo
        .find_reference("HEAD")
        .map_err(|_| "output HEAD unavailable")?;
    if head.kind() != Some(ReferenceType::Symbolic) {
        return Err("detached output repositories are not adopted");
    }
    let head_target = head
        .symbolic_target()
        .map_err(|_| "output HEAD is not text")?
        .ok_or("output HEAD target unavailable")?;
    let reference = format!("refs/heads/{branch}");
    let tracking = format!("refs/remotes/origin/{branch}");
    match expected {
        OutputCheckoutState::Missing | OutputCheckoutState::Unborn => {
            if !head_target
                .strip_prefix("refs/heads/")
                .is_some_and(gazette_core::source::valid_branch_name)
            {
                return Err("unborn output HEAD is not a literal branch");
            }
            if repo
                .references()
                .map_err(|_| "output refs unavailable")?
                .next()
                .is_some()
            {
                return Err("unborn output contains existing refs");
            }
            if empty_objects {
                let mut has_objects = false;
                let result = repo.odb().and_then(|odb| {
                    odb.foreach(|_| {
                        has_objects = true;
                        false
                    })
                });
                if has_objects {
                    return Err("unborn output contains existing objects");
                }
                result.map_err(|_| "unborn output objects cannot be inspected")?;
            }
            inspect_index(repo, None)?;
        }
        OutputCheckoutState::Commit { oid } => {
            if head_target != reference {
                return Err("output HEAD differs from the configured branch");
            }
            let oid = Oid::from_str(oid.as_str()).map_err(|_| "output pre-state commit invalid")?;
            for entry in repo.references().map_err(|_| "output refs unavailable")? {
                let entry = entry.map_err(|_| "output ref cannot be read")?;
                if !matches!(entry.name(),Ok(name) if name==reference || name==tracking) {
                    return Err("output contains unrelated refs");
                }
            }
            for name in [&reference, &tracking] {
                let value = repo
                    .find_reference(name)
                    .map_err(|_| "output tracked ref missing")?;
                if value.kind() != Some(ReferenceType::Direct) || value.target() != Some(oid) {
                    return Err("output tracked ref changed from selected pre-state");
                }
            }
        }
    }
    Ok(())
}

fn inspect_index(repo: &Repository, parent: Option<&gazette_core::source::CommitId>) -> Result<()> {
    let index = repo.index().map_err(|_| "output index unavailable")?;
    if index.has_conflicts() {
        return Err("output index has conflicts");
    }
    let mut expected = Index::new().map_err(|_| "isolated output index unavailable")?;
    if let Some(parent) = parent {
        let oid = Oid::from_str(parent.as_str()).map_err(|_| "output index parent invalid")?;
        let tree = repo
            .find_commit(oid)
            .and_then(|c| c.tree())
            .map_err(|_| "output parent tree unavailable")?;
        expected
            .read_tree(&tree)
            .map_err(|_| "output parent index cannot be read")?;
    }
    if index.len() != expected.len()
        || index.iter().zip(expected.iter()).any(|(a, b)| {
            a.path != b.path
                || a.id != b.id
                || a.mode != b.mode
                || a.flags_extended != 0
                || a.flags & 0x3000 != 0
        })
    {
        return Err("output index differs from the selected parent tree");
    }
    Ok(())
}

fn descendant(
    source: &ParentSource,
    new: &gazette_core::source::CommitId,
    old: &gazette_core::source::CommitId,
    budget: &mut MetadataBudget,
) -> Result<()> {
    if new == old {
        return Ok(());
    }
    let selected = match source {
        ParentSource::Bare { path } => super::SelectedRepository::open(path, budget)?,
        ParentSource::Worktree { path } => super::SelectedRepository::open_worktree(path, budget)?,
    };
    let new = Oid::from_str(new.as_str()).map_err(|_| "output parent invalid")?;
    let old = Oid::from_str(old.as_str()).map_err(|_| "output pre-state invalid")?;
    if selected.repo.graph_descendant_of(new, old).ok() != Some(true) {
        return Err("output update is not a verified fast-forward");
    }
    selected.recheck()
}

fn inspect(
    repo: &Repository,
    _remote: &str,
    branch: &str,
    parent: Option<&gazette_core::source::CommitId>,
    configuration: &str,
    administrative: &Path,
    budget: &mut MetadataBudget,
) -> Result<()> {
    if setup::read(&administrative.join("config"), budget)? != configuration.as_bytes() {
        return Err("output tracking configuration changed");
    }
    let state = parent.map_or(OutputCheckoutState::Unborn, |oid| {
        OutputCheckoutState::Commit { oid: oid.clone() }
    });
    prestate(repo, &state, branch, true)?;
    inspect_index(repo, parent)
}
