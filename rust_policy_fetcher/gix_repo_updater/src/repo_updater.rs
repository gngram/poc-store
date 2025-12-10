use std::path::PathBuf;
use gix;
use gix::bstr::ByteSlice;
use gix::object::tree::diff::{Action, Change};
use tracing::{info};
use anyhow::{Context, Result};

/* RepoUpdater structure */
pub struct RepoUpdater {
    pub url: String,
    pub branch: String,
    pub destination: PathBuf,
    pub remote_name: String,

    repo: Option<gix::Repository>,
    repo_head: Option<gix::hash::ObjectId>,
}

impl RepoUpdater {
    pub fn new<U: Into<String>, B: Into<String>, P: Into<PathBuf>>(
        url: U,
        branch: B,
        destination: P,
    ) -> Result<Self> {
        Self::new_inner(url, branch, destination, "origin")
    }

    fn new_inner<U: Into<String>, B: Into<String>, P: Into<PathBuf>, R: Into<String>>(
        url: U,
        branch: B,
        destination: P,
        remote: R,
    ) -> Result<Self> {
        let mut updater = Self {
            url: url.into(),
            branch: branch.into(),
            destination: destination.into(),
            remote_name: remote.into(),
            repo: None,
            repo_head: None,

        };

        /* Attempt to load and validate policies from the existing repository */
        if updater.destination.exists() {
            match gix::open(&updater.destination) {
                Ok(repo) => {
                    /*
                      Validate branch and URL match from config.
                      Branch is fixed to avoid any merge conflict.
                    */
                    let head_ref = repo.head()?;
                    let _current_branch = head_ref
                        .referent_name()
                        .map(|r| r.shorten().to_string())
                        .unwrap_or_default();

                    let remote_url = repo
                        .config_snapshot()
                        .string("remote.origin.url")
                        .map(|s| s.to_string())
                        .unwrap_or_default();

                    if remote_url == updater.url {
                        info!(
                            "Successfully loaded existing repository from '{}'",
                            updater.destination.display()
                        );
                        let head = repo.head_id()?;
                        updater.repo_head = Some(head.detach());
                        updater.repo = Some(repo);
                        return Ok(updater);
                    } else {
                        info!(
                            "Repository at '{}' is not from provided source. Re-cloning...",
                            updater.destination.display()
                        );
                    }
                }
                Err(_) => {
                    info!(
                        "Path '{}' exists but is not a valid git repository. Re-cloning...",
                        updater.destination.display()
                    );
                }
            }

            /* Clean up invalid directory (if exists) before cloning */
            std::fs::remove_dir_all(&updater.destination).with_context(|| {
                format!(
                    "Failed to delete directory '{}'",
                    updater.destination.display()
                )
            })?;
        }

        updater.clone_repo()?;
        Ok(updater)
    }

    fn clone_repo(&mut self) -> Result<()> {
        info!("Cloning repository from: {}", self.url);
        info!("Branch: {}", self.branch);
        info!("Destination: {:?}", self.destination);

        let interrupt = &gix::interrupt::IS_INTERRUPTED;

        let mut prepare = gix::prepare_clone(self.url.as_str(), &self.destination)?
            .with_ref_name(Some(self.branch.as_str()))?;

        let (mut checkout, _fetch_outcome) =
            prepare.fetch_then_checkout(gix::progress::Discard, interrupt)?;

        let (repo, _checkout_outcome) =
            checkout.main_worktree(gix::progress::Discard, interrupt)?;

        let head = repo.head_id()?;
        self.repo_head = Some(head.detach());
        self.repo = Some(repo);

        info!("Repository cloned successfully.");
        info!("Checked out HEAD: {}", self.repo_head.as_ref().unwrap());
        Ok(())
    }

    pub fn repo_head(&self) -> Option<gix::hash::ObjectId> {
        self.repo_head
    }

    fn fetch(&self) -> Result<()> {
        let repo = self.repo.as_ref().context("Repo should be initialized")?;
        let remote_name = self.remote_name.as_str();
        let remote = repo.find_remote(remote_name)?;

        let mut progress = gix::progress::Discard;
        let _fetch_outcome = remote
            .connect(gix::remote::Direction::Fetch)?
            .prepare_fetch(&mut progress, Default::default())?
            .receive(progress, &gix::interrupt::IS_INTERRUPTED)?;
        Ok(())
    }

    fn checkout(&mut self, commit_id: gix::hash::ObjectId) -> Result<()> {
        let repo = self.repo.as_ref().context("Repo should be initialized")?;
        let local_branch = format!("refs/heads/{}", self.branch);
        let remote_name = self.remote_name.as_str();

        // Create or update local branch
        match repo.find_reference(&local_branch) {
            Ok(mut branch_ref) => {
                // Branch exists, update it
                branch_ref.set_target_id(commit_id, "fast-forward from remote")?;
            }
            Err(_) => {
                // Create new branch
                repo.reference(
                    local_branch.as_str(),
                    commit_id,
                    gix::refs::transaction::PreviousValue::MustNotExist,
                    format!("branch from {}/{}", remote_name, self.branch),
                )?;
            }
        }

        // Update HEAD to point to the branch symbolically
        std::fs::write(
            repo.git_dir().join("HEAD"),
            format!("ref: {}\n", local_branch)
        )?;

        // Perform checkout to update working directory
        let commit = repo.find_object(commit_id)?.into_commit();
        let tree = commit.tree()?;

        // Checkout the tree to the working directory
        let mut index = repo.index_from_tree(&tree.id)?;
        let opts = gix::worktree::state::checkout::Options {
            overwrite_existing: true,
            ..Default::default()
        };
        let objects = repo.objects.clone().into_arc()?;

        gix::worktree::state::checkout(
            &mut index,
            repo.workdir().context("Repository has no working directory")?,
            objects,
            &gix::progress::Discard,
            &gix::progress::Discard,
            &gix::interrupt::IS_INTERRUPTED,
            opts,
        )?;

        // Write the index to disk
        index.write(gix::index::write::Options::default())?;

        // Update repo_head to the new commit
        self.repo_head = Some(commit_id);
        info!("Checked out HEAD: {}", commit_id);
        Ok(())
    }

    pub fn pull(&mut self) -> Result<Option<gix::hash::ObjectId>> {
        if self.repo.is_none() {
            self.clone_repo()?;
        }
        // Store the old head for comparison
        let old_head = self.repo_head;

        self.fetch()?;

        let commit_id = {
            let repo = self.repo.as_ref().context("Repo should be initialized")?;
            let remote_tracking = format!("refs/remotes/{}/{}", self.remote_name, self.branch);
            let remote_ref = repo.find_reference(&remote_tracking)?;
            remote_ref.id().detach()
        };

        self.checkout(commit_id)?;

        // Return the new head if it changed, otherwise None
        if old_head != self.repo_head {
            Ok(old_head)
        } else {
            Ok(None)
        }
    }

    pub fn get_change_set(&self, from_rev: &str, to_rev: &str) -> Result<String> {
        let repo = self.repo.as_ref().context(
            "Repository not loaded. Call clone_repo or load_from_path first.",
        )?;

        info!("Diffing {} -> {}", from_rev, to_rev);

        let from_tree = repo
            .rev_parse_single(from_rev)?
            .object()?
            .peel_to_tree()?;

        let to_tree = repo
            .rev_parse_single(to_rev)?
            .object()?
            .peel_to_tree()?;

        let mut changes_str = String::new();

        from_tree
            .changes()?
            .for_each_to_obtain_tree(&to_tree, |change| {
                let line = match change {
                    Change::Modification { location, .. } => {
                        format!("M  {}\n", location.to_str_lossy())
                    }
                    Change::Addition { location, .. } => {
                        format!("A  {}\n", location.to_str_lossy())
                    }
                    Change::Deletion { location, .. } => {
                        format!("D  {}\n", location.to_str_lossy())
                    }
                    _ => String::new(),
                };
                changes_str.push_str(&line);

                Ok::<_, std::convert::Infallible>(Action::Continue)
            })?;

        Ok(changes_str)
    }
}
