//! A download of the whole database, as a consistent SQLite file.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use tokio::io::AsyncReadExt;

use super::{require, ApiError, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;
use crate::store::{now, AuditEntry, Dialect, POSTGRES_BACKUP_TEXT};

/// The file is read in pieces of this size.
const CHUNK: usize = 64 * 1024;

/// Removes the temporary file when it is dropped: when the download ended,
/// when the caller went away, or when anything before it failed.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Downloads a consistent copy of the database: a SQLite file with every
/// table, taken of one moment while the gateway goes on. It holds
/// everything the database holds (users with their password hashes,
/// sessions, logs, provider credentials as they are stored) except the
/// master key, which is not in it: the credentials are unreadable without
/// that key, and so the copy is of little use without it. Admin only; the
/// download is audited. On PostgreSQL there is no file to give: 409
/// `backup_unsupported`, with the advice to use `pg_dump`.
#[utoipa::path(
    get,
    path = "/backup",
    tag = "backup",
    operation_id = "backup_download",
    responses(
        (status = 200, description = "The database as a SQLite file (`application/vnd.sqlite3`; also declared as `application/octet-stream` so that generated clients return the bytes).", content((inline(super::openapi::BinaryBody) = "application/vnd.sqlite3"), (inline(super::openapi::BinaryBody) = "application/octet-stream"))),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "The database is PostgreSQL, which is backed up with pg_dump (code backup_unsupported).", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn download(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageSettings)?;
    if state.store.dialect() == Dialect::Postgres {
        return Err(ApiError::conflict(
            "backup_unsupported",
            POSTGRES_BACKUP_TEXT,
        ));
    }

    // In the data directory, which only the owner can enter. An in-memory
    // database (tests) has none: the system's temporary directory is used.
    let dir = state
        .store
        .data_dir()
        .map_or_else(std::env::temp_dir, std::path::Path::to_path_buf);
    let mut random = [0u8; 8];
    crate::secrets::fill_random(&mut random);
    let temp = TempFile(dir.join(format!(".ultrafast-backup-{}.tmp", hex::encode(random))));
    state.store.backup_to(&temp.0).await?;
    restrict(&temp.0)?;
    let mut file = tokio::fs::File::open(&temp.0)
        .await
        .map_err(anyhow::Error::from)?;
    let length = file.metadata().await.map_err(anyhow::Error::from)?.len();
    // Nothing of the copy stays on the disk while it is sent: what is open
    // can still be read.
    #[cfg(unix)]
    let _ = std::fs::remove_file(&temp.0);

    let mut tx = state.store.begin().await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "backup.download",
        target_type: "backup",
        target_id: None,
        summary: "Downloaded a backup of the database",
    })
    .await?;
    tx.commit().await?;

    let stream = async_stream::stream! {
        // Held until the stream ends or is dropped.
        let _temp = temp;
        let mut buffer = vec![0u8; CHUNK];
        loop {
            match file.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buffer[..n])),
                Err(e) => {
                    yield Err(e);
                    break;
                }
            }
        }
    };
    // `YYYY-MM-DD HH:MM:SS` as `YYYYMMDD-HHMMSS`.
    let stamp: String = now()
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ' ')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect();
    Ok((
        [
            (CONTENT_TYPE, "application/vnd.sqlite3".to_string()),
            (CONTENT_LENGTH, length.to_string()),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"ultrafast-{stamp}.db\""),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

/// The copy is for the owner of the data directory alone.
#[cfg(unix)]
fn restrict(path: &std::path::Path) -> Result<(), ApiError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(anyhow::Error::from)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_path: &std::path::Path) -> Result<(), ApiError> {
    Ok(())
}
