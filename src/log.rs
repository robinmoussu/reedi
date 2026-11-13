//! The commits displayed by `reedi`: those `git log $base..HEAD -p --reverse` displays, where
//! `$base` is described by [`Base`].

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::Path;

use gix::ObjectId;
use gix::attrs::StateRef;
use gix::bstr::BStr;
use gix::bstr::ByteSlice;
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
use gix::traverse::commit::simple::CommitTimeOrder;
use gix::worktree::stack::state::attributes::Source;
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
use crate::settings::CONTEXT;

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

/// The commits to display: the fake commits with `--fake`, otherwise the commits of the
/// repository containing `directory` (see the [module documentation](self)).
pub fn displayed_commits(cli: &Cli, directory: &Path) -> Result<Vec<Commit>, LogError> {
    if cli.fake {
        return Ok(fake::all());
    }
    let repository = gix::discover(directory).map_err(|error| {
        if error.is_not_found() {
            LogError::NotARepository
        } else {
            unreadable(error)
        }
    })?;
    commits_of(&repository)
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
    let mut attributes = DiffAttributes::new(repository)?;
    ids.into_iter()
        .rev()
        .map(|id| {
            let decorations = decorations.remove(&id).unwrap_or_default();
            commit(repository, id, decorations, &mut attributes)
        })
        .collect()
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
    let is_merge = commit.parent_ids().nth(1).is_some();
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
        subject: message.summary().to_str_lossy().into_owned(),
        author: author.name.to_str_lossy().into_owned(),
        email: author.email.to_str_lossy().into_owned(),
        date: author.time().map_err(unreadable)?,
        decorations,
        body: message.body.map_or_else(String::new, |body| {
            without_leading_blank_lines(body)
                .to_str_lossy()
                .into_owned()
        }),
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
    if [before, after]
        .iter()
        .flatten()
        .any(|(_, mode, _)| !mode.is_blob_or_symlink())
    {
        return Ok(None);
    }
    let content_of = |file: Option<TreeFile>| {
        file.map(|(_, _, id)| {
            repository
                .find_blob(id)
                .map(|mut blob| blob.take_data())
                .map_err(unreadable)
        })
        .transpose()
    };
    let (before_content, after_content) = (content_of(before)?, content_of(after)?);
    let diff_as_of = |attributes: &mut DiffAttributes, file: Option<TreeFile>| {
        file.map_or(Ok(DiffAs::Auto), |(path, _, _)| attributes.diff_as(path))
    };
    let diff_as = DiffAs::of_versions(
        diff_as_of(attributes, before)?,
        diff_as_of(attributes, after)?,
    );
    Ok(Some(diff::file(
        state(before, &before_content),
        state(after, &after_content),
        diff_as,
        CONTEXT,
    )))
}

/// The version `file` of a file, whose content is `content`, if the version exists.
fn state<'a>(file: Option<TreeFile<'a>>, content: &'a Option<Vec<u8>>) -> Option<FileState<'a>> {
    file.zip(content.as_deref())
        .map(|((path, _, _), content)| FileState { path, content })
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
    use crate::test_git::Git;
    use crate::test_git::RebaseBackend;

    /// A branch in parallel with `BRANCH`.
    const SIDE_BRANCH: &str = "side";

    fn hashes(commits: &[Commit]) -> Vec<String> {
        commits
            .iter()
            .map(|commit| commit.hash.to_string())
            .collect()
    }

    /// The only item of `items`.
    fn single<T>(items: &[T]) -> &T {
        let [item] = items else {
            panic!("{} items instead of 1", items.len());
        };
        item
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
        git.stdout_of(&["init", "--quiet", "submodule"]);
        git.stdout_of(&[
            "-C",
            "submodule",
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-gpg-sign",
            "--message=In the submodule",
        ]);
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
        let commits = displayed_commits(&cli, directory.path()).unwrap();
        assert_eq!(hashes(&commits), hashes(&fake::all()));
    }

    #[test]
    fn without_fake_option_the_commits_of_the_repository_are_displayed() {
        let git = Git::with_upstream();
        git.commit_all(b"Unpushed");
        let cli = Cli::parse_from(["reedi"]);
        let commits = displayed_commits(&cli, git.path()).unwrap();
        assert_eq!(hashes(&commits), git.log_reverse("origin/topic..HEAD"));
    }
}
