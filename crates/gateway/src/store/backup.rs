//! A consistent copy of the database, taken while the gateway writes.

use std::path::Path;

use anyhow::{bail, Result};

use super::Store;

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
        sqlx::query("VACUUM INTO ?")
            .bind(target)
            .execute(self.pool())
            .await?;
        Ok(())
    }
}
