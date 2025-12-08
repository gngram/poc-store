use gix::bstr::ByteSlice;
use std::path::PathBuf;
use gix;
use std::sync::atomic::AtomicBool;
use anyhow::{anyhow, Context, Result};
use gix::{progress, remote::Direction};
use gix::object::tree::diff::{Action, Change};
use tracing::{info};

/* RepoUpdater structure */
pub struct RepoUpdater {
    pub url: String,
    pub branch: String,
    pub destination: PathBuf,

    repo: Option<gix::Repository>,
    repo_head: Option<gix::hash::ObjectId>,
}

impl RepoUpdater {
    pub fn new<U: Into<String>, B: Into<String>, P: Into<PathBuf>>(
        url: U,
        branch: B,
        destination: P,
    ) -> Result<Self> {
        let mut updater = Self {
            url: url.into(),
            branch: branch.into(),
            destination: destination.into(),
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
                    let current_branch = head_ref
                        .referent_name()
                        .map(|r| r.shorten().to_string())
                        .unwrap_or_default();

                    let remote_url = repo
                        .config_snapshot()
                        .string("remote.origin.url")
                        .map(|s| s.to_string())
                        .unwrap_or_default();

                    if current_branch == updater.branch && remote_url == updater.url {
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
                            "Repository at '{}' is invalid (branch/URL mismatch). Re-cloning...",
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

    pub fn pull(&mut self) -> Result<Option<gix::hash::ObjectId>> {
        let repo = self.repo.as_mut().context(
            "Repository not loaded. Call clone_repo or load_from_path first.",
        )?;

        let mut remote =
            repo.find_remote("origin").context("Remote 'origin' not found")?;

        info!("Fetching origin/{}...", self.branch);

        /* Fetch branch HEAD from remote */
        let refspec = format!(
            "+refs/heads/{}:refs/remotes/origin/{}",
            self.branch, self.branch
        );
        remote.replace_refspecs(Some(refspec.as_str()), Direction::Fetch)
              .expect("static refspec must be valid");

        let mut fetch_progress = progress::Discard;
        remote
            .connect(Direction::Fetch)?
            .prepare_fetch(&mut fetch_progress, Default::default())?
            .receive(&mut fetch_progress, &gix::interrupt::IS_INTERRUPTED)?;

        /* Compare remote and local commit id */
        let local_id = self
            .repo_head
            .context("Internal state error: repo_head is not set. Call clone_repo or load_from_path first.")?;

        let remote_ref_name = format!("refs/remotes/origin/{}", self.branch);
        let remote_ref = repo.find_reference(&remote_ref_name)?;
        let remote_id = remote_ref.id().detach();

        if local_id == remote_id {
            info!("Already up to date.");
            return Ok(None);
        }

        /* Before fast-forward check local must be ancestor of remote */
        info!("Verifying ancestry...");
        let base = repo.merge_base(local_id, remote_id)?;
        if base != local_id {
            return Err(anyhow!(
                "Update rejected: local {} is not an ancestor of remote {} (history diverged)",
                local_id,
                remote_id
            ));
        }

        /* Move local branch ref to remote commit */
        info!("Fast-forwarding {} to {}", self.branch, remote_id);
        let mut local_ref =
            repo.find_reference(&format!("refs/heads/{}", self.branch))?;
        local_ref.set_target_id(remote_id, "pull: fast-forward only")?;

        use gix::worktree::state::checkout as worktree_checkout;

        let workdir = repo
            .workdir()
            .context("No worktree found (is this a bare repository?)")?;

        let tree_id = repo
            .find_object(remote_id)?
            .peel_to_kind(gix::object::Kind::Tree)?
            .id;
        let mut index = repo.index_from_tree(&tree_id)?;

        let mut checkout_progress = progress::Discard;
        let mut attributes_progress = progress::Discard;
        let should_interrupt = AtomicBool::new(false);
        let objects = repo.objects.clone().into_arc()?;

        worktree_checkout(
            &mut index,
            workdir,
            objects,
            &mut checkout_progress,
            &mut attributes_progress,
            &should_interrupt,
            gix::worktree::state::checkout::Options {
                destination_is_initially_empty: false,
                overwrite_existing: true,
                ..Default::default()
            },
        )?;

        let index_path = repo.index_path();
        let mut file = gix::lock::File::acquire_to_update_resource(
            &index_path,
            gix::lock::acquire::Fail::Immediately,
            None,
        )?;
        index.write_to(&mut file, gix::index::write::Options::default())?;
        file.commit()?;

        self.repo_head = Some(remote_id);

        info!("Success: {} is now at {}", self.branch, remote_id);
        Ok(Some(local_id))
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
