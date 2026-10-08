//! A consistent copy of the database, taken while the gateway writes.

use std::path::Path;

use anyhow::{bail, Result};

use super::dialect::Dialected;
use super::Store;

/// What is answered where a backup of a PostgreSQL database is asked for:
/// the gateway does not copy it.
pub const POSTGRES_BACKUP_TEXT: &str = "Use pg_dump to back up a Postgres database.";

impl Store {
    /// The directory the database file is in; `None` for an in-memory
    /// database.
    pub fn data_dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Writes a copy of the database to `path`, which must not exist. It is
    /// SQLite's online backup (`VACUUM INTO`): one read transaction, so the
    /// copy is of one moment, whatever is written meanwhile, and it holds
    /// no write lock. The copy holds everything the database holds except
    /// the master key, which is not in it: provider credentials in the copy
    /// are unreadable without that key.
    pub async fn backup_to(&self, path: &Path) -> Result<()> {
        if self.dialect != super::Dialect::Sqlite {
            bail!("{POSTGRES_BACKUP_TEXT}");
        }
        if self.dir.is_none() {
            // SQLite writes nothing for an in-memory database.
            bail!("an in-memory database cannot be backed up");
        }
        if path.exists() {
            bail!("{} already exists", path.display());
        }
        let Some(target) = path.to_str() else {
            bail!("the path of a backup must be text");
        };
        // Made here, readable by its owner alone, so that the copy is never
        // readable by others while it is written (`VACUUM INTO` takes an
        // empty file that exists).
        create_private(path)?;
        let done = self
            .q("VACUUM INTO ?")
            .bind(target)
            .execute(self.pool())
            .await;
        if let Err(e) = done {
            let _ = std::fs::remove_file(path);
            return Err(e.into());
        }
        Ok(())
    }
}

/// Creates `path`, which must not exist, with mode 0600 where there are modes.
fn create_private(path: &Path) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map(drop)
        .map_err(|e| anyhow::anyhow!("could not create {}: {e}", path.display()))
}
