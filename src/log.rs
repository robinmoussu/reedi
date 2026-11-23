//! The revisions displayed by `reedi`: the index and the working directory if the working
//! directory is dirty, otherwise the commits `git log $base..HEAD -p --reverse` displays, where
//! `$base` is described by [`Base`].

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::io::Read;
use std::panic;
use std::path::Path;
use std::thread;

use gix::ObjectId;
use gix::attrs::StateRef;
use gix::bstr::BStr;
use gix::bstr::BString;
use gix::bstr::ByteSlice;
use gix::filter::plumbing::pipeline::convert::ToGitOutcome;
use gix::hash::Prefix;
use gix::head;
use gix::object::tree::diff::ChangeDetached;
use gix::objs::commit::MessageRef;
use gix::objs::tree::EntryMode;
use gix::prelude::ObjectIdExt;
use gix::refs::Category;
use gix::refs::FullName;
use gix::refs::PartialNameRef;
use gix::remote::Direction;
use gix::revision::walk::Sorting;
use gix::status::Submodule;
use gix::status::UntrackedFiles;
use gix::status::index_worktree;
use gix::status::plumbing::index_as_worktree::Change;
use gix::status::plumbing::index_as_worktree::EntryStatus;
use gix::status::tree_index::TrackRenames;
use gix::submodule::config::Ignore;
use gix::traverse::commit::simple::CommitTimeOrder;
use gix::worktree::stack::state::attributes::Source;
use reedi_gix_compat::message;
use reedi_gix_compat::rebase;
use reedi_gix_compat::upstream;
use reediif::diff;
use reediif::diff::DiffAs;
use reediif::diff::FileState;
use reediif::model::FileDiff;

use crate::cli::Cli;
use crate::fake;
use crate::model::Commit;
use crate::model::Decoration;
use crate::model::Index;
use crate::model::NextCommit;
use crate::model::Revision;
use crate::model::WorkingDirectory;
use crate::settings::CONTEXT;

/// The files that may hold the message prepared for the next commit, relative to the git
/// directory, in the order they are looked for: the message of a merge (also written by a
/// cherry-pick or a rebase stopped by a conflict), of a squash, and of the commit a rebase
/// replays.
const NEXT_COMMIT_MESSAGE_FILES: [&str; 3] = ["MERGE_MSG", "SQUASH_MSG", "rebase-merge/message"];

/// The attribute deciding whether a file is diffed as text or as binary (`binary` unsets it).
const DIFF_ATTRIBUTE: &str = "diff";

/// The commit whose ancestors (itself included) are not displayed.
#[derive(Clone)]
pub enum Base {
    /// During a rebase: the commit the commits are replayed onto, by its short hash.
    Onto(Prefix),
    /// The upstream of the local branch HEAD is attached to.
    Upstream {
        /// The local branch (`refs/heads/main`).
        branch: FullName,
        /// The upstream, on its remote (`branch.<name>.merge`, like `refs/heads/main`).
        remote_branch: FullName,
        /// The upstream, in this repository (`refs/remotes/origin/main`, or `refs/heads/main` if
        /// the upstream is a local branch).
        tracking_branch: FullName,
    },
    /// The HEAD of the default remote, since HEAD has no upstream.
    RemoteHead {
        /// `refs/remotes/<remote>/HEAD`.
        remote_head: FullName,
        head: head::Kind,
    },
}

/// A base, and the commit it designates.
struct BaseCommit {
    base: Base,
    commit_id: ObjectId,
}

/// The commits cannot be read.
pub enum LogError {
    /// The directory is in no git repository.
    NotARepository,
    /// HEAD is on a branch without commits.
    UnbornBranch(FullName),
    /// HEAD has no upstream, and the default remote has no HEAD.
    NoBase {
        head: head::Kind,
        /// `refs/remotes/<remote>/HEAD`, which doesn't exist, `None` if there is no default remote
        /// (see `gix::Repository::remote_default_name()`).
        remote_head: Option<FullName>,
    },
    /// The upstream of the branch HEAD is attached to is not in the repository.
    UpstreamNotFetched {
        branch: FullName,
        /// The upstream, in this repository (`refs/remotes/origin/main`).
        tracking_branch: FullName,
    },
    /// HEAD is `Base` (or one of its ancestors).
    NoCommits(Base),
    /// The repository cannot be read.
    Unreadable(String),
}

/// The local branch HEAD is on, if it is not detached.
fn branch_of(head: &head::Kind) -> Option<&FullName> {
    match head {
        head::Kind::Symbolic(reference) => Some(&reference.name),
        head::Kind::Unborn(branch) => Some(branch),
        head::Kind::Detached { .. } => None,
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LogError::NotARepository => write!(f, "not in a git repository"),
            LogError::UnbornBranch(branch) => write!(f, "no commits on {}", branch.shorten()),
            LogError::NoBase { head, remote_head } => {
                let missing = match remote_head {
                    Some(remote_head) => remote_head.shorten().to_string(),
                    None => "default remote".to_string(),
                };
                match branch_of(head) {
                    Some(branch) => write!(
                        f,
                        "branch {} has no upstream and there is no {missing}",
                        branch.shorten()
                    ),
                    None => write!(f, "HEAD is detached and there is no {missing}"),
                }
            }
            LogError::UpstreamNotFetched { branch, tracking_branch } => write!(
                f,
                "the upstream {} of branch {} was never fetched",
                tracking_branch.shorten(),
                branch.shorten()
            ),
            LogError::NoCommits(Base::Onto(onto)) => {
                write!(f, "no commits replayed yet onto {onto}")
            }
            LogError::NoCommits(Base::Upstream { branch, remote_branch, tracking_branch }) => {
                if branch.shorten() == remote_branch.shorten() {
                    write!(f, "no unpushed commits on {}", branch.shorten())
                } else {
                    write!(
                        f,
                        "no commits on {} that are not on {}",
                        branch.shorten(),
                        tracking_branch.shorten()
                    )
                }
            }
            LogError::NoCommits(Base::RemoteHead { remote_head, head }) => {
                let head = branch_of(head).map_or("HEAD".into(), |branch| branch.shorten());
                write!(
                    f,
                    "no commits on {head} that are not on {}",
                    remote_head.shorten()
                )
            }
            LogError::Unreadable(reason) => write!(f, "cannot read the repository: {reason}"),
        }
    }
}

/// Like `Display`, since `head::Kind` has no `Debug`.
impl fmt::Debug for LogError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for LogError {}

fn unreadable(error: impl fmt::Display) -> LogError {
    LogError::Unreadable(error.to_string())
}

/// The revisions to display: the fake commits with `--fake`, otherwise the commits of the
/// repository containing `directory` (see the [module documentation](self)).
pub fn displayed_revisions(cli: &Cli, directory: &Path) -> Result<Vec<Revision>, LogError> {
    if cli.fake {
        return Ok(fake::all().into_iter().map(Revision::Commit).collect());
    }
    if cli.fake_dirty_directory {
        let (index, working_directory) = fake::dirty_directory();
        return Ok(vec![
            Revision::Index(index),
            Revision::WorkingDirectory(working_directory),
        ]);
    }
    let repository = gix::discover(directory).map_err(|error| {
        if error.is_not_found() {
            LogError::NotARepository
        } else {
            unreadable(error)
        }
    })?;
    if let Some((index, working_directory)) = uncommitted_changes_of(&repository)? {
        return Ok(vec![
            Revision::Index(index),
            Revision::WorkingDirectory(working_directory),
        ]);
    }
    Ok(commits_of(&repository)?
        .into_iter()
        .map(Revision::Commit)
        .collect())
}

/// The index and the working directory of `repository`, if the working directory is dirty (a
/// tracked file differs between HEAD and the index, or between the index and the working
/// directory).
fn uncommitted_changes_of(
    repository: &gix::Repository,
) -> Result<Option<(Index, WorkingDirectory)>, LogError> {
    // On a branch without commits, the tree of HEAD is the empty tree.
    let head_tree = repository
        .head_tree_id_or_empty()
        .map_err(unreadable)?
        .object()
        .map_err(unreadable)?
        .into_tree();
    let index = repository.index_or_empty().map_err(unreadable)?;
    let mut attributes = DiffAttributes::new(repository)?;
    let index_files = staged_files(repository, &head_tree, &index, &mut attributes)?;
    let working_directory_files = unstaged_files(repository, &head_tree, &index, &mut attributes)?;
    if index_files.is_empty() && working_directory_files.is_empty() {
        return Ok(None);
    }
    let next_commit = if rebase::onto(repository.git_dir())
        .map_err(unreadable)?
        .is_some()
    {
        NextCommit::RebaseContinue
    } else {
        branch_of(&repository.head().map_err(unreadable)?.kind)
            .cloned()
            .map_or(NextCommit::DetachedHead, NextCommit::Branch)
    };
    let message = next_commit_message(repository)?;
    let (subject, body) = message.map_or_else(Default::default, |message| {
        subject_and_body(MessageRef::from_bytes(&message))
    });
    let index = Index {
        next_commit,
        subject,
        body,
        files: index_files,
    };
    let working_directory = WorkingDirectory { files: working_directory_files };
    Ok(Some((index, working_directory)))
}

/// The content of `path`, or `None` if it doesn't exist.
fn read_if_exists(path: &Path) -> Result<Option<Vec<u8>>, LogError> {
    match fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(unreadable(error)),
    }
}

/// The message prepared for the next commit of `repository`, without its comment lines: the first
/// existing file of `NEXT_COMMIT_MESSAGE_FILES`, then the file of `commit.template`. `None` if there
/// is none.
fn next_commit_message(repository: &gix::Repository) -> Result<Option<BString>, LogError> {
    let prepared = NEXT_COMMIT_MESSAGE_FILES
        .iter()
        .map(|file| read_if_exists(&repository.git_dir().join(file)))
        .find_map(Result::transpose)
        .transpose()?;
    let message = match prepared {
        Some(message) => Some(message),
        None => {
            let template = repository
                .config_snapshot()
                .trusted_path("commit.template")
                .map_err(unreadable)?;
            match template {
                Some(template) => read_if_exists(&template)?,
                None => None,
            }
        }
    };
    Ok(message.map(|message| message::stripped(repository, message.as_bstr())))
}

/// The diff of each file whose version in `index` differs from the one in `head_tree`, like `git
/// diff --cached`, sorted by path.
fn staged_files(
    repository: &gix::Repository,
    head_tree: &gix::Tree,
    index: &gix::index::State,
    attributes: &mut DiffAttributes,
) -> Result<Vec<FileDiff>, LogError> {
    // Like `git diff`, renames are detected according to `diff.renames`, and by default.
    let (rewrites, is_configured) = gix::diff::new_rewrites(
        repository.config_snapshot().plumbing(),
        /* lenient */ true,
    )
    .map_err(unreadable)?;
    let renames = match (rewrites, is_configured) {
        (Some(rewrites), _) => TrackRenames::Given(rewrites),
        (None, false) => TrackRenames::Given(Default::default()),
        (None, true) => TrackRenames::Disabled,
    };
    let mut changes = Vec::new();
    repository
        .tree_index_status(&head_tree.id, index, None, renames, |change, _, _| {
            changes.push(change.into_owned());
            Ok(std::ops::ControlFlow::Continue(()))
        })
        .map_err(unreadable)?;
    let mut files = changes
        .iter()
        .filter_map(|change| staged_file_diff(repository, change, attributes).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    // The unmerged entries are not compared, they have one entry per side of the conflict.
    let mut unmerged_paths: Vec<_> = index
        .entries()
        .iter()
        .filter(|entry| entry.stage() != gix::index::entry::Stage::Unconflicted)
        .map(|entry| entry.path(index))
        .collect();
    unmerged_paths.dedup();
    files.extend(unmerged_paths.into_iter().map(diff::unmerged));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// The version of a file in an index, as a version of a file in a tree.
fn tree_file<'a>(
    path: &'a BStr,
    mode: gix::index::entry::Mode,
    id: &gix::oid,
) -> Option<TreeFile<'a>> {
    Some((path, mode.to_tree_entry_mode()?, id.to_owned()))
}

/// The diff of the file modified by `change` (from the tree of HEAD to the index), `None` if it
/// is not displayed (submodules, and changes of the mode only).
fn staged_file_diff(
    repository: &gix::Repository,
    change: &gix::diff::index::Change,
    attributes: &mut DiffAttributes,
) -> Result<Option<FileDiff>, LogError> {
    use gix::diff::index::ChangeRef;
    let (before, after) = match change {
        ChangeRef::Addition { location, entry_mode, id, .. } => {
            (None, tree_file(location, *entry_mode, id))
        }
        ChangeRef::Deletion { location, entry_mode, id, .. } => {
            (tree_file(location, *entry_mode, id), None)
        }
        ChangeRef::Modification {
            location,
            previous_entry_mode,
            previous_id,
            entry_mode,
            id,
            ..
        } => {
            if previous_id == id {
                return Ok(None);
            }
            (
                tree_file(location, *previous_entry_mode, previous_id),
                tree_file(location, *entry_mode, id),
            )
        }
        ChangeRef::Rewrite {
            source_location,
            source_entry_mode,
            source_id,
            location,
            entry_mode,
            id,
            copy,
            ..
        } => {
            let after = tree_file(location, *entry_mode, id);
            if *copy {
                (None, after)
            } else {
                (
                    tree_file(source_location, *source_entry_mode, source_id),
                    after,
                )
            }
        }
    };
    blobs_diff(repository, before, after, attributes)
}

/// The diff of each file whose version in the working directory differs from the one in `index`,
/// like `git diff`, sorted by path. Untracked files are ignored, and an unmerged file is compared
/// to its version in `head_tree`.
fn unstaged_files(
    repository: &gix::Repository,
    head_tree: &gix::Tree,
    index: &gix::index::State,
    attributes: &mut DiffAttributes,
) -> Result<Vec<FileDiff>, LogError> {
    let (mut pipeline, _) = repository.filter_pipeline(None).map_err(unreadable)?;
    let items = repository
        .status(gix::progress::Discard)
        .map_err(unreadable)?
        .untracked_files(UntrackedFiles::None)
        .index_worktree_submodules(Submodule::Given { ignore: Ignore::All, check_dirty: false })
        .index_worktree_rewrites(None)
        .into_index_worktree_iter(Vec::new())
        .map_err(unreadable)?;
    let mut files = items
        .map(|item| {
            let item = item.map_err(unreadable)?;
            unstaged_file_diff(
                repository,
                head_tree,
                index,
                &mut pipeline,
                &item,
                attributes,
            )
        })
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// The diff of the file of `item` (a difference between the index and the working directory),
/// `None` if it is not displayed (submodules, and changes of the mode only).
fn unstaged_file_diff(
    repository: &gix::Repository,
    head_tree: &gix::Tree,
    index: &gix::index::State,
    pipeline: &mut gix::filter::Pipeline,
    item: &index_worktree::Item,
    attributes: &mut DiffAttributes,
) -> Result<Option<FileDiff>, LogError> {
    let index_worktree::Item::Modification { entry, rela_path, status, .. } = item else {
        return Ok(None);
    };
    let path = rela_path.as_bstr();
    let in_index = || tree_file(path, entry.mode, &entry.id);
    let (before, after_exists) = match status {
        EntryStatus::Conflict { .. } => {
            let in_head = head_tree
                .lookup_entry_by_path(gix::path::from_bstr(path).map_err(unreadable)?)
                .map_err(unreadable)?
                .map(|in_head| (path, in_head.mode(), in_head.object_id()));
            (in_head, true)
        }
        EntryStatus::IntentToAdd => (None, true),
        EntryStatus::Change(Change::Removed) => (in_index(), false),
        EntryStatus::Change(
            Change::Type { .. } | Change::Modification { content_change: Some(_), .. },
        ) => (in_index(), true),
        EntryStatus::Change(Change::Modification { content_change: None, .. })
        | EntryStatus::Change(Change::SubmoduleModification(_))
        | EntryStatus::NeedsUpdate(_) => return Ok(None),
    };
    if before.is_some_and(|(_, mode, _)| !mode.is_blob_or_symlink()) {
        return Ok(None);
    }
    let before = before
        .map(|file| blob_version(repository, file))
        .transpose()?;
    let after = if after_exists {
        worktree_version(repository, pipeline, index, path)?
    } else {
        None
    };
    diff_of(before, after, attributes).map(Some)
}

/// The version in the working directory of the file at `path`, converted like `git diff` does
/// (`core.autocrlf`, `text` and `eol` attributes, clean filters), `None` if it doesn't exist.
fn worktree_version(
    repository: &gix::Repository,
    pipeline: &mut gix::filter::Pipeline,
    index: &gix::index::State,
    path: &BStr,
) -> Result<Option<Version>, LogError> {
    let relative_path = gix::path::from_bstr(path).map_err(unreadable)?;
    let workdir = repository
        .workdir()
        .ok_or_else(|| unreadable("the repository has no working directory"))?;
    let absolute_path = workdir.join(&relative_path);
    let metadata = match fs::symlink_metadata(&absolute_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(error)),
    };
    // Like git, a symbolic link is diffed as its target.
    let content = if metadata.is_symlink() {
        let target = fs::read_link(&absolute_path).map_err(unreadable)?;
        gix::path::into_bstr(target)
            .map_err(unreadable)?
            .into_owned()
            .into()
    } else {
        let file = fs::File::open(&absolute_path).map_err(unreadable)?;
        let mut content = Vec::new();
        match pipeline
            .convert_to_git(file, &relative_path, index)
            .map_err(unreadable)?
        {
            ToGitOutcome::Unchanged(mut reader) => reader.read_to_end(&mut content),
            ToGitOutcome::Process(mut reader) => reader.read_to_end(&mut content),
            ToGitOutcome::Buffer(buffer) => {
                content.extend_from_slice(buffer);
                Ok(buffer.len())
            }
        }
        .map_err(unreadable)?;
        content
    };
    Ok(Some(Version { path: path.to_owned(), content }))
}

/// The commits of `repository` (see the [module documentation](self)).
fn commits_of(repository: &gix::Repository) -> Result<Vec<Commit>, LogError> {
    let head = repository.head().map_err(unreadable)?;
    let Some(head_id) = head.id().map(|id| id.detach()) else {
        let head::Kind::Unborn(branch) = head.kind else {
            unreachable!("only an unborn HEAD has no commit");
        };
        return Err(LogError::UnbornBranch(branch));
    };
    let mut decorations = decorations_by_commit(repository, &head.kind, head_id)?;
    let BaseCommit { base, commit_id: base_id } = base_of(repository, head.kind)?;
    let walk = repository
        .rev_walk([head_id])
        .with_hidden([base_id])
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(unreadable)?;
    let ids = walk
        .map(|info| info.map(|info| info.id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(unreadable)?;
    if ids.is_empty() {
        return Err(LogError::NoCommits(base));
    }
    let decorated: Vec<_> = ids
        .into_iter()
        .rev()
        .map(|id| (id, decorations.remove(&id).unwrap_or_default()))
        .collect();
    // The commits are loaded by contiguous chunks, one per thread, so that the blobs shared by
    // consecutive commits are read by the same thread.
    let chunk_len = decorated.len().div_ceil(threads_count());
    let sync_repository = repository.clone().into_sync();
    thread::scope(|scope| {
        let threads: Vec<_> = decorated
            .chunks(chunk_len)
            .map(|chunk| {
                let sync_repository = &sync_repository;
                scope.spawn(move || {
                    let mut repository = sync_repository.to_thread_local();
                    // The version of a file after a commit is the version before the next one:
                    // its blob is decompressed once.
                    repository.object_cache_size_if_unset(OBJECT_CACHE_SIZE);
                    let mut attributes = DiffAttributes::new(&repository)?;
                    chunk
                        .iter()
                        .map(|(id, decorations)| {
                            commit(&repository, *id, decorations.clone(), &mut attributes)
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .collect();
        // The first error is the one of the first commit that can't be loaded.
        let chunks = threads
            .into_iter()
            .map(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|panic| panic::resume_unwind(panic))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(chunks.into_iter().flatten().collect())
    })
}

/// The size of the cache of the objects of each thread loading the commits, in bytes.
const OBJECT_CACHE_SIZE: usize = 64 * 1024 * 1024;

/// The number of threads loading the commits: the available parallelism.
fn threads_count() -> usize {
    thread::available_parallelism().map_or(1, |count| count.get())
}

/// The base of the commits of `repository` whose HEAD is `head`.
fn base_of(repository: &gix::Repository, head: head::Kind) -> Result<BaseCommit, LogError> {
    if let Some(onto) = rebase::onto(repository.git_dir()).map_err(unreadable)? {
        let short_hash = onto.attach(repository).shorten().map_err(unreadable)?;
        return Ok(BaseCommit {
            base: Base::Onto(short_hash),
            commit_id: onto,
        });
    }
    if let head::Kind::Symbolic(reference) = &head
        && let Some(upstream) = upstream_of(repository, &reference.name)?
    {
        return Ok(upstream);
    }
    let Some(remote) = repository.remote_default_name(Direction::Fetch) else {
        return Err(LogError::NoBase { head, remote_head: None });
    };
    let remote_head = Category::RemoteBranch
        .to_full_name([remote.as_slice(), b"/HEAD"].concat().as_bstr())
        .map_err(unreadable)?;
    match commit_of_reference(repository, remote_head.as_ref().into())? {
        Some(commit_id) => Ok(BaseCommit {
            base: Base::RemoteHead { remote_head, head },
            commit_id,
        }),
        None => Err(LogError::NoBase { head, remote_head: Some(remote_head) }),
    }
}

/// The upstream of `branch` and its commit, if it is set.
///
/// Fails if the upstream is not in the repository.
fn upstream_of(
    repository: &gix::Repository,
    branch: &FullName,
) -> Result<Option<BaseCommit>, LogError> {
    let Some(remote_branch) = repository
        .branch_remote_ref_name(branch.as_ref(), Direction::Fetch)
        .transpose()
        .map_err(unreadable)?
    else {
        return Ok(None);
    };
    let Some(tracking_branch) = upstream::tracking_branch(repository, branch.as_ref())
        .transpose()
        .map_err(unreadable)?
    else {
        return Ok(None);
    };
    let Some(upstream) = commit_of_reference(repository, tracking_branch.as_ref().into())? else {
        return Err(LogError::UpstreamNotFetched { branch: branch.clone(), tracking_branch });
    };
    let base = Base::Upstream {
        branch: branch.clone(),
        remote_branch,
        tracking_branch,
    };
    Ok(Some(BaseCommit { base, commit_id: upstream }))
}

/// The commit the reference `name` points to, if it exists.
fn commit_of_reference(
    repository: &gix::Repository,
    name: &PartialNameRef,
) -> Result<Option<ObjectId>, LogError> {
    let Some(mut reference) = repository.try_find_reference(name).map_err(unreadable)? else {
        return Ok(None);
    };
    let id = reference.peel_to_id().map_err(unreadable)?;
    Ok(Some(id.detach()))
}

/// The decorations of the commits of `repository`, whose HEAD is `head` at `head_id`, in the order
/// of `git log --format=%D`: HEAD first, then the other references by name, in reverse order.
///
/// References that cannot be resolved to a commit are ignored, like git.
fn decorations_by_commit(
    repository: &gix::Repository,
    head: &head::Kind,
    head_id: ObjectId,
) -> Result<HashMap<ObjectId, Vec<Decoration>>, LogError> {
    let head_branch = branch_of(head);
    let mut references = repository
        .references()
        .map_err(unreadable)?
        .all()
        .map_err(unreadable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unreadable)?;
    references.sort_by(|left, right| right.name().as_bstr().cmp(left.name().as_bstr()));
    let mut decorations: HashMap<ObjectId, Vec<Decoration>> = HashMap::new();
    let head_decoration =
        Decoration::Head(head_branch.map(|branch| branch.shorten().to_str_lossy().into_owned()));
    decorations.insert(head_id, vec![head_decoration]);
    for mut reference in references {
        if Some(&reference.name().to_owned()) == head_branch {
            continue;
        }
        let Some((category, short_name)) = reference.name().category_and_short_name() else {
            continue;
        };
        let short_name = short_name.to_str_lossy().into_owned();
        let decoration = match category {
            Category::LocalBranch => Decoration::LocalBranch(short_name),
            Category::RemoteBranch => Decoration::RemoteBranch(short_name),
            Category::Tag => Decoration::Tag(short_name),
            _ => continue,
        };
        if let Ok(id) = reference.peel_to_id() {
            decorations.entry(id.detach()).or_default().push(decoration);
        }
    }
    Ok(decorations)
}

/// `text` without its leading lines that only contain whitespace, like git does for `%b`.
fn without_leading_blank_lines(text: &BStr) -> &BStr {
    let blank_len: usize = text
        .lines_with_terminator()
        .take_while(|line| line.trim().is_empty())
        .map(<[u8]>::len)
        .sum();
    text[blank_len..].as_bstr()
}

/// The subject of `message`, as `git log --format=%s` displays it, and its body, as `%b` displays
/// it.
fn subject_and_body(message: MessageRef) -> (String, String) {
    let subject = message.summary().to_str_lossy().into_owned();
    let body = message.body.map_or_else(String::new, |body| {
        without_leading_blank_lines(body)
            .to_str_lossy()
            .into_owned()
    });
    (subject, body)
}

/// The commit `id` of `repository`, decorated with `decorations`.
fn commit(
    repository: &gix::Repository,
    id: ObjectId,
    decorations: Vec<Decoration>,
    attributes: &mut DiffAttributes,
) -> Result<Commit, LogError> {
    let commit = repository.find_commit(id).map_err(unreadable)?;
    let author = commit.author().map_err(unreadable)?;
    let message = MessageRef::from_bytes(commit.message_raw().map_err(unreadable)?);
    let (subject, body) = subject_and_body(message);
    let parent_ids: Vec<_> = commit.parent_ids().map(|id| id.detach()).collect();
    let is_merge = parent_ids.len() > 1;
    // Like `git log -p`, merge commits have no diff.
    let files = if is_merge {
        Vec::new()
    } else {
        files_of(repository, &commit, attributes)?
    };
    Ok(Commit {
        hash: id,
        hash_abbrev_len: id
            .attach(repository)
            .shorten()
            .map_err(unreadable)?
            .hex_len(),
        subject,
        author: author.name.to_str_lossy().into_owned(),
        email: author.email.to_str_lossy().into_owned(),
        date: author.time().map_err(unreadable)?,
        decorations,
        body,
        parent_ids,
        files,
    })
}

/// The diff of each file modified by `commit` (which has at most 1 parent), sorted by path like
/// git.
fn files_of(
    repository: &gix::Repository,
    commit: &gix::Commit,
    attributes: &mut DiffAttributes,
) -> Result<Vec<FileDiff>, LogError> {
    let tree = commit.tree().map_err(unreadable)?;
    let parent_tree = commit
        .parent_ids()
        .next()
        .map(|parent| {
            parent
                .object()
                .map_err(unreadable)?
                .into_commit()
                .tree()
                .map_err(unreadable)
        })
        .transpose()?;
    // The options are read from the configuration (`diff.renames`).
    let changes = repository
        .diff_tree_to_tree(parent_tree.as_ref(), &tree, /* options */ None)
        .map_err(unreadable)?;
    let mut files = changes
        .iter()
        .filter_map(|change| file_diff(repository, change, attributes).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// A version of a file in a tree: its path, mode and object.
type TreeFile<'a> = (&'a BStr, EntryMode, ObjectId);

/// The diff of the file modified by `change`, `None` if it is not displayed (trees, submodules,
/// and changes of the mode only).
fn file_diff(
    repository: &gix::Repository,
    change: &ChangeDetached,
    attributes: &mut DiffAttributes,
) -> Result<Option<FileDiff>, LogError> {
    let (before, after): (Option<TreeFile>, Option<TreeFile>) = match change {
        ChangeDetached::Addition { location, entry_mode, id, .. } => {
            (None, Some((location.as_bstr(), *entry_mode, *id)))
        }
        ChangeDetached::Deletion { location, entry_mode, id, .. } => {
            (Some((location.as_bstr(), *entry_mode, *id)), None)
        }
        ChangeDetached::Modification {
            location,
            previous_entry_mode,
            previous_id,
            entry_mode,
            id,
        } => {
            if previous_id == id {
                return Ok(None);
            }
            (
                Some((location.as_bstr(), *previous_entry_mode, *previous_id)),
                Some((location.as_bstr(), *entry_mode, *id)),
            )
        }
        ChangeDetached::Rewrite {
            source_location,
            source_entry_mode,
            source_id,
            entry_mode,
            id,
            location,
            copy,
            ..
        } => {
            let after = Some((location.as_bstr(), *entry_mode, *id));
            if *copy {
                (None, after)
            } else {
                (
                    Some((source_location.as_bstr(), *source_entry_mode, *source_id)),
                    after,
                )
            }
        }
    };
    blobs_diff(repository, before, after, attributes)
}

/// The diff of a file from `before` to `after` (`None` if it doesn't exist in that version), read
/// from the objects of `repository`, `None` if it is not displayed (trees and submodules).
fn blobs_diff(
    repository: &gix::Repository,
    before: Option<TreeFile>,
    after: Option<TreeFile>,
    attributes: &mut DiffAttributes,
) -> Result<Option<FileDiff>, LogError> {
    if [before, after]
        .iter()
        .flatten()
        .any(|(_, mode, _)| !mode.is_blob_or_symlink())
    {
        return Ok(None);
    }
    let version_of =
        |file: Option<TreeFile>| file.map(|file| blob_version(repository, file)).transpose();
    diff_of(version_of(before)?, version_of(after)?, attributes).map(Some)
}

/// A version of a file: its path and its content.
struct Version {
    path: BString,
    content: Vec<u8>,
}

impl Version {
    fn state(&self) -> FileState<'_> {
        FileState {
            path: self.path.as_bstr(),
            content: &self.content,
        }
    }
}

/// The version `file` of a file, read from the objects of `repository`.
fn blob_version(
    repository: &gix::Repository,
    (path, _, id): TreeFile,
) -> Result<Version, LogError> {
    let content = repository
        .find_blob(id)
        .map(|mut blob| blob.take_data())
        .map_err(unreadable)?;
    Ok(Version { path: path.to_owned(), content })
}

/// The diff of a file from `before` to `after` (`None` if it doesn't exist in that version).
fn diff_of(
    before: Option<Version>,
    after: Option<Version>,
    attributes: &mut DiffAttributes,
) -> Result<FileDiff, LogError> {
    let diff_as_of = |attributes: &mut DiffAttributes, version: &Option<Version>| {
        version.as_ref().map_or(Ok(DiffAs::Auto), |version| {
            attributes.diff_as(version.path.as_bstr())
        })
    };
    let diff_as = DiffAs::of_versions(
        diff_as_of(attributes, &before)?,
        diff_as_of(attributes, &after)?,
    );
    Ok(diff::file(
        before.as_ref().map(Version::state),
        after.as_ref().map(Version::state),
        diff_as,
        CONTEXT,
    ))
}

/// The attributes of the files of a repository that decide how they are diffed, read from its
/// `.gitattributes` files, `.git/info/attributes` and `core.attributesFile`, like git.
struct DiffAttributes<'repo> {
    stack: gix::AttributeStack<'repo>,
}

impl<'repo> DiffAttributes<'repo> {
    fn new(repository: &'repo gix::Repository) -> Result<Self, LogError> {
        let index = repository.index_or_empty().map_err(unreadable)?;
        let stack = repository
            .attributes_only(&index, Source::WorktreeThenIdMapping)
            .map_err(unreadable)?;
        Ok(DiffAttributes { stack })
    }

    /// How the file at `path` is diffed according to its `diff` attribute: the binary detection of
    /// git decides if it is unspecified.
    fn diff_as(&mut self, path: &BStr) -> Result<DiffAs, LogError> {
        let mut outcome = self.stack.selected_attribute_matches([DIFF_ATTRIBUTE]);
        let platform = self
            .stack
            .at_entry(path, /* mode */ None)
            .map_err(unreadable)?;
        platform.matching_attributes(&mut outcome);
        let state = outcome
            .iter_selected()
            .next()
            .map(|matched| matched.assignment.state);
        Ok(match state {
            Some(StateRef::Unset) => DiffAs::Binary,
            // A diff driver (`diff=$driver`) diffs the file as text, unless configured otherwise.
            Some(StateRef::Set | StateRef::Value(_)) => DiffAs::Text,
            Some(StateRef::Unspecified) | None => DiffAs::Auto,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    use clap::Parser;
    use gix::bstr::BStr;
    use gix::bstr::BString;
    use gix::date::time::format;
    use reediif::diff;
    use reediif::diff::DiffAs;
    use reediif::diff::FileState;
    use reediif::model::FileChange;
    use reediif::model::FileContentType;
    use reediif::model::FileDiff;
    use reediif::model::Hunk;

    use super::*;
    use crate::fake;
    use crate::model::Decoration;
    use crate::settings::CONTEXT;
    use crate::test_git::BRANCH;
    use crate::test_git::CONFLICTING_FILE;
    use crate::test_git::Git;
    use crate::test_git::RebaseBackend;
    use crate::test_support::single;
    use reediif::diff::unmerged;

    /// A branch in parallel with `BRANCH`.
    const SIDE_BRANCH: &str = "side";

    fn hashes(commits: &[Commit]) -> Vec<String> {
        commits
            .iter()
            .map(|commit| commit.hash.to_string())
            .collect()
    }

    /// The commits displayed as `revisions`.
    fn commits(revisions: Vec<Revision>) -> Vec<Commit> {
        revisions
            .into_iter()
            .map(|revision| match revision {
                Revision::Commit(commit) => commit,
                other => panic!("{other:?} is not a commit"),
            })
            .collect()
    }

    /// The parallel branches have interleaved dates, so that the order of `git log` (by date)
    /// differs from the topological order.
    #[test]
    fn commits_are_those_of_git_log_reverse_from_the_upstream() {
        let git = Git::with_upstream();
        git.commit_all(b"First on topic");
        git.switch_to_new_branch(SIDE_BRANCH, Some("HEAD~1"));
        git.commit_all(b"First on side");
        git.switch_to(BRANCH);
        git.commit_all(b"Second on topic");
        git.stdout_of(&[
            "merge",
            "--quiet",
            "--no-edit",
            "--no-ff",
            "--no-gpg-sign",
            SIDE_BRANCH,
        ]);
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("origin/topic..HEAD"));
    }

    #[test]
    fn commits_are_in_order_when_more_numerous_than_the_threads_loading_them() {
        let git = Git::with_origin_head();
        for index in 0..2 * threads_count() + 1 {
            git.commit_all(format!("Commit {index}").as_bytes());
        }
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("origin/HEAD..HEAD"));
    }

    /// `origin/HEAD` is at the first commit, the upstream at the second one.
    #[test]
    fn upstream_is_preferred_to_origin_head() {
        let git = Git::with_origin_head();
        git.commit_all(b"Pushed");
        git.set_upstream_at_head();
        git.commit_all(b"Unpushed");
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("origin/topic..HEAD"));
    }

    #[test]
    fn upstream_can_be_a_local_branch() {
        let git = Git::on_branch();
        git.commit_all(b"Base");
        git.stdout_of(&["branch", "local"]);
        git.set_config("branch.topic.remote", ".");
        git.set_config("branch.topic.merge", "refs/heads/local");
        git.commit_all(b"Not on local");
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("local..HEAD"));
    }

    #[test]
    fn without_upstream_commits_are_those_after_origin_head() {
        let git = Git::with_origin_head();
        git.commit_all(b"First");
        git.commit_all(b"Second");
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("origin/HEAD..HEAD"));
    }

    /// The only remote is the default one.
    #[test]
    fn without_upstream_commits_are_those_after_the_head_of_the_default_remote() {
        let git = Git::on_branch();
        git.commit_all(b"Base");
        git.set_remote_head_at_head("upstream");
        git.commit_all(b"Not on upstream");
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("upstream/HEAD..HEAD"));
    }

    /// The last commit is only on the detached HEAD.
    #[test]
    fn with_detached_head_commits_are_those_after_origin_head() {
        let git = Git::with_origin_head();
        git.commit_all(b"On topic");
        git.detach_head();
        git.commit_all(b"On the detached HEAD");
        let commits = commits_of(&git.repository()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("origin/HEAD..HEAD"));
        assert_eq!(
            commits.last().unwrap().decorations,
            [Decoration::Head(None)]
        );
    }

    /// The upstream is before the commit the commits are replayed onto, so that it would display
    /// different commits.
    #[test]
    fn during_a_rebase_commits_are_those_replayed_onto() {
        for backend in [RebaseBackend::Merge, RebaseBackend::Apply] {
            let git = Git::with_upstream();
            let onto =
                git.start_rebase_stopped_by_conflict(backend, /* replayed_commits_count */ 2);
            let commits = commits_of(&git.repository()).unwrap();
            assert_eq!(
                hashes(&commits),
                git.log_reverse(&format!("{onto}..HEAD")),
                "{backend:?}"
            );
        }
    }

    /// The subject has 2 lines, which `%s` joins, and the short hash is longer than the default
    /// `core.abbrev`.
    #[test]
    fn metadata_is_that_of_git_log() {
        let git = Git::with_upstream();
        git.set_config("core.abbrev", "12");
        git.commit_all(b"Subject\ncontinued\n\nFirst paragraph.\n\nSecond paragraph.\n");
        let commits = commits_of(&git.repository()).unwrap();
        let commit = single(&commits);
        let field = |placeholder: &str| {
            git.stdout_of(&[
                "log",
                "-1",
                "--date=raw",
                &format!("--format=format:{placeholder}"),
            ])
        };
        assert_eq!(commit.hash.to_string(), field("%H"));
        assert_eq!(commit.short_hash().to_string(), field("%h"));
        assert_eq!(commit.subject, field("%s"));
        assert_eq!(commit.body, field("%b"));
        assert_eq!(commit.author, field("%an"));
        assert_eq!(commit.email, field("%ae"));
        assert_eq!(commit.date.format(format::RAW).unwrap(), field("%ad"));
    }

    #[test]
    fn invalid_utf8_in_the_metadata_is_replaced() {
        let git = Git::with_upstream();
        git.commit_with_raw_message(b"Caf\xe9\n\nBody \xff\n");
        let commits = commits_of(&git.repository()).unwrap();
        let commit = single(&commits);
        assert_eq!(commit.subject, "Caf\u{FFFD}");
        assert_eq!(commit.body, "Body \u{FFFD}\n");
    }

    /// `v2` is an annotated tag, and `refs/custom/` holds neither branches nor tags.
    #[test]
    fn decorations_are_head_branches_and_tags_in_the_order_of_git_log() {
        let git = Git::with_upstream();
        let decorated = git.commit_all(b"Decorated");
        git.stdout_of(&["tag", "v1"]);
        git.stdout_of(&["tag", "--annotate", "--no-sign", "--message=v2", "v2"]);
        git.stdout_of(&["branch", "other"]);
        git.set_remote_branch("fork", "wip", &decorated);
        git.stdout_of(&["update-ref", "refs/custom/ignored", &decorated]);
        git.commit_all(b"HEAD");
        let commits = commits_of(&git.repository()).unwrap();
        let decorations: Vec<_> = commits.iter().map(|commit| &commit.decorations).collect();
        assert_eq!(
            decorations,
            [
                &vec![
                    Decoration::Tag("v2".to_string()),
                    Decoration::Tag("v1".to_string()),
                    Decoration::RemoteBranch("fork/wip".to_string()),
                    Decoration::LocalBranch("other".to_string()),
                ],
                &vec![Decoration::Head(Some(BRANCH.to_string()))],
            ]
        );
    }

    /// What is compared of a file diff (`FileDiff` has no `PartialEq`): its path, change, content
    /// type and hunks.
    type ComparableFile = (BString, FileChange, FileContentType, Vec<Hunk>);

    fn comparable(file: &FileDiff) -> ComparableFile {
        (
            file.path.clone(),
            file.change.clone(),
            file.content,
            file.hunks.clone(),
        )
    }

    fn comparable_files(files: &[FileDiff]) -> Vec<ComparableFile> {
        files.iter().map(comparable).collect()
    }

    /// The version of a file at `path` whose content is `content`.
    fn state<'a>(path: &'a str, content: &'a [u8]) -> Option<FileState<'a>> {
        Some(FileState { path: BStr::new(path), content })
    }

    /// The diff of a file from `before` to `after` (`None` if it doesn't exist in that version),
    /// computed like the diffs of the fake commits, from the content of the file.
    fn file_diff(before: Option<FileState>, after: Option<FileState>) -> FileDiff {
        diff::file(before, after, DiffAs::Auto, CONTEXT)
    }

    /// Each file has a different change, with the content git needs to detect it.
    #[test]
    fn files_are_those_of_git_log_p_sorted_by_path() {
        let modified_before = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let modified_after = b"1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
        let renamed = b"Renamed without modification.\n";
        let link = |git: &Git, target| {
            let path = git.path().join("link");
            let _ = fs::remove_file(&path);
            symlink(target, path).unwrap();
        };
        let git = Git::on_branch();
        git.write("deleted.txt", b"Deleted.\n");
        git.write("modified.txt", modified_before);
        git.write("old.txt", renamed);
        git.write("mode.sh", b"Its mode changes.\n");
        link(&git, "before");
        git.commit_all(b"Before");
        git.set_upstream_at_head();

        git.write("added.txt", b"Added.\n");
        fs::remove_file(git.path().join("deleted.txt")).unwrap();
        git.write("modified.txt", modified_after);
        fs::rename(git.path().join("old.txt"), git.path().join("new.txt")).unwrap();
        fs::set_permissions(
            git.path().join("mode.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        link(&git, "after");
        git.write("nul.bin", b"NUL\0byte\n");
        git.write("latin1.txt", b"Caf\xe9\n");
        git.commit_in_submodule("submodule");
        git.commit_all(b"After");

        let commits = commits_of(&git.repository()).unwrap();
        let expected = [
            file_diff(None, state("added.txt", b"Added.\n")),
            file_diff(state("deleted.txt", b"Deleted.\n"), None),
            file_diff(None, state("latin1.txt", b"Caf\xe9\n")),
            file_diff(state("link", b"before"), state("link", b"after")),
            file_diff(
                state("modified.txt", modified_before),
                state("modified.txt", modified_after),
            ),
            file_diff(state("old.txt", renamed), state("new.txt", renamed)),
            file_diff(None, state("nul.bin", b"NUL\0byte\n")),
        ];
        assert_eq!(
            comparable_files(&single(&commits).files),
            comparable_files(&expected)
        );
    }

    #[test]
    fn renames_are_not_detected_if_disabled_in_the_configuration() {
        let content = b"Renamed without modification.\n";
        let git = Git::on_branch();
        git.write("old.txt", content);
        git.commit_all(b"Before");
        git.set_upstream_at_head();
        git.set_config("diff.renames", "false");
        fs::rename(git.path().join("old.txt"), git.path().join("new.txt")).unwrap();
        git.commit_all(b"Rename");
        let commits = commits_of(&git.repository()).unwrap();
        let expected = [
            file_diff(None, state("new.txt", content)),
            file_diff(state("old.txt", content), None),
        ];
        assert_eq!(
            comparable_files(&single(&commits).files),
            comparable_files(&expected)
        );
    }

    /// The content of the file is text, so only its attributes make it binary.
    #[test]
    fn attributes_of_the_repository_make_files_binary() {
        const ATTRIBUTES: &[u8] = b"forced.txt binary\n";
        /// The file of `core.attributesFile`, relative to the working tree.
        const ATTRIBUTES_FILE: &str = ".git/custom_attributes";
        /// A source of attributes, and how to set `ATTRIBUTES` in it.
        type AttributesSource = (&'static str, fn(&Git));
        let sources: [AttributesSource; 3] = [
            (".gitattributes", |git| {
                git.write(".gitattributes", ATTRIBUTES)
            }),
            (".git/info/attributes", |git| {
                git.write(".git/info/attributes", ATTRIBUTES)
            }),
            ("core.attributesFile", |git| {
                git.write(ATTRIBUTES_FILE, ATTRIBUTES);
                let path = git.path().join(ATTRIBUTES_FILE);
                git.set_config("core.attributesFile", path.to_str().unwrap());
            }),
        ];
        for (source, set_attributes) in sources {
            let git = Git::on_branch();
            set_attributes(&git);
            git.commit_all(b"Attributes");
            git.set_upstream_at_head();
            git.write("forced.txt", b"Text.\n");
            git.commit_all(b"Forced");
            let commits = commits_of(&git.repository()).unwrap();
            let file = single(&single(&commits).files);
            assert_eq!(file.content, FileContentType::Binary, "{source}");
        }
    }

    /// The content of the file has a NUL byte, so only its attributes make it text.
    #[test]
    fn diff_attribute_makes_files_text() {
        let content = b"NUL\0byte\n";
        let git = Git::on_branch();
        git.write(".gitattributes", b"forced.txt diff\n");
        git.commit_all(b"Attributes");
        git.set_upstream_at_head();
        git.write("forced.txt", content);
        git.commit_all(b"Forced");
        let commits = commits_of(&git.repository()).unwrap();
        let expected = diff::file(
            /* before */ None,
            /* after */ state("forced.txt", content),
            DiffAs::Text,
            CONTEXT,
        );
        assert_eq!(
            comparable_files(&single(&commits).files),
            comparable_files(&[expected])
        );
    }

    /// The directory is not a repository, so reading it would fail.
    #[test]
    fn fake_option_displays_the_fake_commits() {
        let directory = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["reedi", "--fake"]);
        let commits = commits(displayed_revisions(&cli, directory.path()).unwrap());
        assert_eq!(hashes(&commits), hashes(&fake::all()));
    }

    /// The directory is not a repository, so reading it would fail.
    #[test]
    fn fake_dirty_directory_option_displays_the_fake_index_and_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["reedi", "--fake-dirty-directory"]);
        let revisions = displayed_revisions(&cli, directory.path()).unwrap();
        assert!(
            matches!(
                revisions[..],
                [Revision::Index(_), Revision::WorkingDirectory(_)]
            ),
            "{revisions:?}"
        );
    }

    #[test]
    fn without_fake_option_the_commits_of_the_repository_are_displayed() {
        let git = Git::with_upstream();
        git.commit_all(b"Unpushed");
        let cli = Cli::parse_from(["reedi"]);
        let commits = commits(displayed_revisions(&cli, git.path()).unwrap());
        assert_eq!(hashes(&commits), git.log_reverse("origin/topic..HEAD"));
    }

    /// The commit parents are those of `git log --format=%P`, in the same order.
    #[test]
    fn parents_are_those_of_git_log() {
        let git = Git::with_upstream();
        git.commit_all(b"First on topic");
        git.switch_to_new_branch(SIDE_BRANCH, Some("HEAD~1"));
        git.commit_all(b"First on side");
        git.switch_to(BRANCH);
        git.stdout_of(&[
            "merge",
            "--quiet",
            "--no-edit",
            "--no-ff",
            "--no-gpg-sign",
            SIDE_BRANCH,
        ]);
        let commits = commits_of(&git.repository()).unwrap();
        let parents: Vec<Vec<String>> = commits
            .iter()
            .map(|commit| commit.parent_ids.iter().map(ObjectId::to_string).collect())
            .collect();
        let expected: Vec<Vec<String>> = commits
            .iter()
            .map(|commit| {
                git.stdout_of(&["log", "-1", "--format=%P", &commit.hash.to_string()])
                    .split_whitespace()
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        assert_eq!(parents, expected);
        // The merge commit.
        assert_eq!(parents.last().unwrap().len(), 2);
    }

    /// The content of `a.txt` in HEAD, in the index and in the working directory of the tests of
    /// the uncommitted changes.
    const HEAD_CONTENT: &[u8] = b"head\n";
    const INDEX_CONTENT: &[u8] = b"index\n";
    const WORKING_DIRECTORY_CONTENT: &[u8] = b"working directory\n";

    /// A repository whose upstream is at HEAD, where `a.txt` is `HEAD_CONTENT`.
    fn with_committed_file() -> Git {
        let git = Git::on_branch();
        git.write("a.txt", HEAD_CONTENT);
        git.commit_all(b"Base");
        git.set_upstream_at_head();
        git
    }

    /// The index and the working directory of `git`, whose working directory must be dirty.
    fn uncommitted_changes(git: &Git) -> (Index, WorkingDirectory) {
        uncommitted_changes_of(&git.repository())
            .unwrap()
            .expect("the working directory is dirty")
    }

    #[test]
    fn staged_changes_are_in_the_index_and_unstaged_ones_in_the_working_directory() {
        let git = with_committed_file();
        git.write("a.txt", INDEX_CONTENT);
        git.stage("a.txt");
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        let (index, working_directory) = uncommitted_changes(&git);
        let staged = file_diff(state("a.txt", HEAD_CONTENT), state("a.txt", INDEX_CONTENT));
        let unstaged = file_diff(
            state("a.txt", INDEX_CONTENT),
            state("a.txt", WORKING_DIRECTORY_CONTENT),
        );
        assert_eq!(comparable_files(&index.files), comparable_files(&[staged]));
        assert_eq!(
            comparable_files(&working_directory.files),
            comparable_files(&[unstaged])
        );
    }

    #[test]
    fn staged_changes_alone_make_the_working_directory_dirty() {
        let git = with_committed_file();
        git.write("a.txt", INDEX_CONTENT);
        git.stage("a.txt");
        let (index, working_directory) = uncommitted_changes(&git);
        assert_eq!(index.files.len(), 1);
        assert!(working_directory.files.is_empty());
    }

    #[test]
    fn unstaged_changes_alone_make_the_working_directory_dirty() {
        let git = with_committed_file();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        let (index, working_directory) = uncommitted_changes(&git);
        assert!(index.files.is_empty());
        assert_eq!(working_directory.files.len(), 1);
    }

    #[test]
    fn clean_working_directory_has_no_uncommitted_changes() {
        let git = with_committed_file();
        assert!(uncommitted_changes_of(&git.repository()).unwrap().is_none());
    }

    #[test]
    fn untracked_files_do_not_make_the_working_directory_dirty() {
        let git = with_committed_file();
        git.write("untracked.txt", b"Untracked.\n");
        assert!(uncommitted_changes_of(&git.repository()).unwrap().is_none());
    }

    #[test]
    fn untracked_files_are_not_displayed() {
        let git = with_committed_file();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        git.write("untracked.txt", b"Untracked.\n");
        let (_, working_directory) = uncommitted_changes(&git);
        let paths: Vec<_> = working_directory
            .files
            .iter()
            .map(|file| &file.path)
            .collect();
        assert_eq!(paths, ["a.txt"]);
    }

    /// The mode changes both in the index and in the working directory.
    #[test]
    fn mode_only_changes_do_not_make_the_working_directory_dirty() {
        let git = with_committed_file();
        git.stdout_of(&["update-index", "--chmod=+x", "a.txt"]);
        fs::set_permissions(git.path().join("a.txt"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(uncommitted_changes_of(&git.repository()).unwrap().is_none());
    }

    #[test]
    fn submodule_changes_do_not_make_the_working_directory_dirty() {
        let git = with_committed_file();
        git.commit_in_submodule("submodule");
        git.commit_all(b"Submodule");
        // Unstaged, then staged.
        git.commit_in_submodule("submodule");
        assert!(uncommitted_changes_of(&git.repository()).unwrap().is_none());
        git.stage("submodule");
        assert!(uncommitted_changes_of(&git.repository()).unwrap().is_none());
    }

    /// The repository has a commit that is not on the upstream.
    #[test]
    fn dirty_working_directory_displays_the_index_then_the_working_directory_instead_of_the_commits()
     {
        let git = with_committed_file();
        git.commit_all(b"Unpushed");
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        let cli = Cli::parse_from(["reedi"]);
        let revisions = displayed_revisions(&cli, git.path()).unwrap();
        assert!(
            matches!(
                revisions[..],
                [Revision::Index(_), Revision::WorkingDirectory(_)]
            ),
            "{revisions:?}"
        );
    }

    /// The repository has neither upstream nor remote.
    #[test]
    fn dirty_working_directory_is_displayed_without_base() {
        let git = Git::on_branch();
        git.write("a.txt", HEAD_CONTENT);
        git.commit_all(b"Base");
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        let cli = Cli::parse_from(["reedi"]);
        assert_eq!(displayed_revisions(&cli, git.path()).unwrap().len(), 2);
    }

    #[test]
    fn on_a_branch_without_commits_the_index_is_compared_to_an_empty_tree() {
        let git = Git::on_branch();
        git.write("a.txt", INDEX_CONTENT);
        git.stage("a.txt");
        let (index, _) = uncommitted_changes(&git);
        assert_eq!(
            comparable_files(&index.files),
            comparable_files(&[file_diff(None, state("a.txt", INDEX_CONTENT))])
        );
    }

    #[test]
    fn next_commit_is_on_the_branch_head_is_attached_to() {
        let git = with_committed_file();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        assert_eq!(
            uncommitted_changes(&git).0.next_commit,
            NextCommit::Branch(Category::LocalBranch.to_full_name(BRANCH).unwrap())
        );
    }

    #[test]
    fn next_commit_of_a_detached_head_is_on_no_branch() {
        let git = with_committed_file();
        git.detach_head();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        assert_eq!(
            uncommitted_changes(&git).0.next_commit,
            NextCommit::DetachedHead
        );
    }

    #[test]
    fn during_a_rebase_the_next_commit_is_made_by_git_rebase_continue() {
        for backend in [RebaseBackend::Merge, RebaseBackend::Apply] {
            let git = Git::with_upstream();
            git.start_rebase_stopped_by_conflict(backend, /* replayed_commits_count */ 1);
            let (index, _) = uncommitted_changes(&git);
            assert_eq!(index.next_commit, NextCommit::RebaseContinue, "{backend:?}");
        }
    }

    #[test]
    fn unmerged_files_are_unmerged_paths_in_the_index() {
        let git = Git::with_upstream();
        git.start_rebase_stopped_by_conflict(
            RebaseBackend::Merge,
            /* replayed_commits_count */ 0,
        );
        let (index, _) = uncommitted_changes(&git);
        assert_eq!(
            comparable_files(&index.files),
            comparable_files(&[unmerged(CONFLICTING_FILE)])
        );
    }

    /// The file on disk has a single conflict, whose markers are added to the version of HEAD.
    #[test]
    fn unmerged_files_are_compared_to_head_in_the_working_directory() {
        let git = Git::with_upstream();
        git.start_rebase_stopped_by_conflict(
            RebaseBackend::Merge,
            /* replayed_commits_count */ 0,
        );
        let head = git.stdout_of(&["show", &format!("HEAD:{CONFLICTING_FILE}")]);
        let disk = fs::read(git.path().join(CONFLICTING_FILE)).unwrap();
        let (index, working_directory) = uncommitted_changes(&git);
        let expected = file_diff(
            state(CONFLICTING_FILE, head.as_bytes()),
            state(CONFLICTING_FILE, &disk),
        );
        assert_eq!(
            comparable_files(&working_directory.files),
            comparable_files(&[expected])
        );
        assert_eq!(working_directory.conflicts_count(&index), 1);
    }

    /// The path of the file of `commit.template`, relative to the working tree.
    const TEMPLATE_FILE: &str = ".git/custom_template";

    /// A conflicting rebase with the merge backend writes `MERGE_MSG` and
    /// `.git/rebase-merge/message`. Each source is removed in turn, to find the next one.
    #[test]
    fn next_commit_message_is_the_first_existing_of_merge_squash_rebase_and_template_messages() {
        let git = Git::with_upstream();
        git.start_rebase_stopped_by_conflict(
            RebaseBackend::Merge,
            /* replayed_commits_count */ 0,
        );
        git.write(".git/MERGE_MSG", b"Merge\n");
        git.write(".git/SQUASH_MSG", b"Squash\n");
        git.write(".git/rebase-merge/message", b"Rebase\n");
        git.write(TEMPLATE_FILE, b"Template\n");
        let template = git.path().join(TEMPLATE_FILE);
        git.set_config("commit.template", template.to_str().unwrap());
        let subject = || uncommitted_changes(&git).0.subject;
        assert_eq!(subject(), "Merge");
        fs::remove_file(git.path().join(".git/MERGE_MSG")).unwrap();
        assert_eq!(subject(), "Squash");
        fs::remove_file(git.path().join(".git/SQUASH_MSG")).unwrap();
        assert_eq!(subject(), "Rebase");
        fs::remove_file(git.path().join(".git/rebase-merge/message")).unwrap();
        assert_eq!(subject(), "Template");
    }

    #[test]
    fn comment_lines_of_the_next_commit_message_are_removed() {
        let git = with_committed_file();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        git.write(
            ".git/MERGE_MSG",
            b"# Comment.\nSubject\n\nBody.\n# Conflicts:\n#\ta.txt\n",
        );
        let (index, _) = uncommitted_changes(&git);
        assert_eq!(index.subject, "Subject");
        assert_eq!(index.body, "Body.\n");
    }

    #[test]
    fn comment_character_is_read_from_the_configuration() {
        let git = with_committed_file();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        git.set_config("core.commentChar", ";");
        git.write(
            ".git/MERGE_MSG",
            b"Subject\n\n; Comment.\n# Not a comment.\n",
        );
        let (index, _) = uncommitted_changes(&git);
        assert_eq!(index.subject, "Subject");
        assert_eq!(index.body, "# Not a comment.\n");
    }

    #[test]
    fn without_next_commit_message_the_index_has_no_subject_nor_body() {
        let git = with_committed_file();
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        let (index, _) = uncommitted_changes(&git);
        assert_eq!(index.subject, "");
        assert_eq!(index.body, "");
    }

    #[test]
    fn staged_renames_are_detected() {
        let git = with_committed_file();
        git.stdout_of(&["mv", "a.txt", "b.txt"]);
        let (index, _) = uncommitted_changes(&git);
        let expected = file_diff(state("a.txt", HEAD_CONTENT), state("b.txt", HEAD_CONTENT));
        assert_eq!(
            comparable_files(&index.files),
            comparable_files(&[expected])
        );
    }

    /// Like `git diff`, which displays them as added.
    #[test]
    fn intent_to_add_files_are_added_in_the_working_directory_only() {
        let git = with_committed_file();
        git.write("b.txt", WORKING_DIRECTORY_CONTENT);
        git.stdout_of(&["add", "--intent-to-add", "b.txt"]);
        let (index, working_directory) = uncommitted_changes(&git);
        assert!(index.files.is_empty());
        assert_eq!(
            comparable_files(&working_directory.files),
            comparable_files(&[file_diff(None, state("b.txt", WORKING_DIRECTORY_CONTENT))])
        );
    }

    /// With `core.autocrlf`, the line endings of the file on disk are converted to those of the
    /// index, so that only the added line differs.
    #[test]
    fn files_of_the_working_directory_are_converted_like_git_diff() {
        let git = with_committed_file();
        git.set_config("core.autocrlf", "true");
        git.write("a.txt", b"head\r\nadded\r\n");
        let (_, working_directory) = uncommitted_changes(&git);
        let expected = file_diff(
            state("a.txt", HEAD_CONTENT),
            state("a.txt", b"head\nadded\n"),
        );
        assert_eq!(
            comparable_files(&working_directory.files),
            comparable_files(&[expected])
        );
    }

    /// The content of the file is text, so only its attributes make it binary.
    #[test]
    fn attributes_make_files_of_the_index_and_of_the_working_directory_binary() {
        let git = Git::on_branch();
        git.write(".gitattributes", b"a.txt binary\n");
        git.write("a.txt", HEAD_CONTENT);
        git.commit_all(b"Base");
        git.set_upstream_at_head();
        git.write("a.txt", INDEX_CONTENT);
        git.stage("a.txt");
        git.write("a.txt", WORKING_DIRECTORY_CONTENT);
        let (index, working_directory) = uncommitted_changes(&git);
        for files in [&index.files, &working_directory.files] {
            assert_eq!(single(files).content, FileContentType::Binary);
        }
    }
}
