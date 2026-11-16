//! Federation: shares exposed by this node, nodes consumed by this node (ADR 0008).

use super::*;

#[derive(Debug, Clone, FromRow)]
pub struct Share {
    pub id: i64,
    pub label: String,
    pub key_hash: String,
    pub permission: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub remote_node_id: Option<String>,
    pub remote_name: Option<String>,
    pub last_seen: Option<i64>,
    pub last_address: Option<String>,
    /// 'invited' | 'pending' | 'active'
    pub status: String,
    pub pairing_expires_at: Option<i64>,
    pub remote_public_key: Option<String>,
    pub requested_at: Option<i64>,
    /// The invitation asks the other node to share back.
    pub reciprocal: bool,
    /// Address of the other node, sent with its access request.
    pub remote_url: Option<String>,
    /// The other node accepted to share back.
    pub remote_shares_back: bool,
    /// Capabilities of the node reading this share (JSON, ADR 0012) and when they were received.
    pub peer_caps: Option<String>,
    pub peer_caps_at: Option<i64>,
}

impl Share {
    /// Share created by this node in answer to a reciprocal invitation (bound to the inviter).
    pub fn is_reverse(&self) -> bool {
        self.key_hash.starts_with("reciprocal:")
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_share(
    db: &Db,
    label: &str,
    key_hash: &str,
    permission: &str,
    expires_at: Option<i64>,
    created_by: &str,
    installation_ids: &[i64],
    reciprocal: bool,
) -> sqlx::Result<Share> {
    let mut tx = db.begin().await?;
    let share: Share = sqlx::query_as(
        "INSERT INTO shares (label, key_hash, permission, expires_at, created_at, created_by, status, pairing_expires_at, reciprocal)
         VALUES (?, ?, ?, ?, ?, ?, 'invited', ?, ?) RETURNING *",
    )
    .bind(label)
    .bind(key_hash)
    .bind(permission)
    .bind(expires_at)
    .bind(now())
    .bind(created_by)
    .bind(now() + crate::federation::PAIRING_TTL_SECS)
    .bind(reciprocal)
    .fetch_one(&mut *tx)
    .await?;
    for id in installation_ids {
        sqlx::query("INSERT INTO share_installations (share_id, installation_id) VALUES (?, ?)").bind(share.id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(share)
}

pub async fn list_shares(db: &Db) -> sqlx::Result<Vec<Share>> {
    sqlx::query_as("SELECT * FROM shares ORDER BY id").fetch_all(db).await
}

pub async fn share_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Share>> {
    sqlx::query_as("SELECT * FROM shares WHERE id = ?").bind(id).fetch_optional(db).await
}

/// Share still waiting for its invitation code to be used.
pub async fn invited_share(db: &Db, code_hash: &str) -> sqlx::Result<Option<Share>> {
    sqlx::query_as("SELECT * FROM shares WHERE key_hash = ? AND status = 'invited'").bind(code_hash).fetch_optional(db).await
}

/// Uses the invitation code (once): records the requesting node and waits for approval.
/// Returns false if the code was used meanwhile.
/// `shares_back_from`: address of the requesting node when it shares back (reciprocal invitation).
#[allow(clippy::too_many_arguments)]
pub async fn claim_share(db: &Db, id: i64, node_id: &str, name: &str, public_key: &str, address: Option<&str>, shares_back_from: Option<&str>) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE shares SET status = 'pending', key_hash = 'used:' || id, remote_node_id = ?, remote_name = ?,
         remote_public_key = ?, last_address = ?, requested_at = ?, remote_url = ?, remote_shares_back = ?
         WHERE id = ? AND status = 'invited'",
    )
    .bind(node_id)
    .bind(name)
    .bind(public_key)
    .bind(address)
    .bind(now())
    .bind(shares_back_from)
    .bind(shares_back_from.is_some())
    .bind(id)
    .execute(db)
    .await?;
    Ok(r.rows_affected() == 1)
}

/// The share a paired node uses (the most recent one if it paired several times).
pub async fn share_of_node(db: &Db, node_id: &str) -> sqlx::Result<Option<Share>> {
    sqlx::query_as("SELECT * FROM shares WHERE remote_node_id = ? AND status IN ('pending', 'active') ORDER BY id DESC LIMIT 1").bind(node_id).fetch_optional(db).await
}

/// Share given back to the node that invited this one (reciprocal invitation): bound to that
/// node's key right away, since this node's administrator chose to share.
#[allow(clippy::too_many_arguments)]
pub async fn insert_reverse_share(
    db: &Db,
    label: &str,
    permission: &str,
    created_by: &str,
    installation_ids: &[i64],
    node_id: &str,
    name: &str,
    public_key: &str,
) -> sqlx::Result<Share> {
    let mut tx = db.begin().await?;
    let share: Share = sqlx::query_as(
        "INSERT INTO shares (label, key_hash, permission, created_at, created_by, status, remote_node_id, remote_name, remote_public_key, reciprocal)
         VALUES (?, ?, ?, ?, ?, 'active', ?, ?, ?, 1) RETURNING *",
    )
    .bind(label)
    .bind(format!("reciprocal:{}", hex::encode(crate::auth::random_bytes::<16>())))
    .bind(permission)
    .bind(now())
    .bind(created_by)
    .bind(node_id)
    .bind(name)
    .bind(public_key)
    .fetch_one(&mut *tx)
    .await?;
    for id in installation_ids {
        sqlx::query("INSERT INTO share_installations (share_id, installation_id) VALUES (?, ?)").bind(share.id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(share)
}

pub async fn approve_share(db: &Db, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("UPDATE shares SET status = 'active' WHERE id = ? AND status = 'pending'").bind(id).execute(db).await?;
    Ok(r.rows_affected() == 1)
}

pub async fn share_installation_ids(db: &Db, share_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar("SELECT installation_id FROM share_installations WHERE share_id = ? ORDER BY installation_id").bind(share_id).fetch_all(db).await
}

pub async fn set_share_installations(db: &Db, share_id: i64, installation_ids: &[i64]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM share_installations WHERE share_id = ?").bind(share_id).execute(&mut *tx).await?;
    for id in installation_ids {
        sqlx::query("INSERT INTO share_installations (share_id, installation_id) VALUES (?, ?)").bind(share_id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn update_share(db: &Db, id: i64, label: &str, permission: &str, expires_at: Option<i64>) -> sqlx::Result<()> {
    sqlx::query("UPDATE shares SET label = ?, permission = ?, expires_at = ? WHERE id = ?").bind(label).bind(permission).bind(expires_at).bind(id).execute(db).await?;
    Ok(())
}

/// Records a call made with a share key (who, from where, when).
pub async fn touch_share(db: &Db, id: i64, remote_node_id: Option<&str>, remote_name: Option<&str>, address: Option<&str>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE shares SET last_seen = ?, last_address = COALESCE(?, last_address),
         remote_node_id = COALESCE(?, remote_node_id), remote_name = COALESCE(?, remote_name) WHERE id = ?",
    )
    .bind(now())
    .bind(address)
    .bind(remote_node_id)
    .bind(remote_name)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_share(db: &Db, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM shares WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow)]
pub struct Node {
    pub id: i64,
    pub remote_node_id: String,
    pub name: String,
    pub url: String,
    pub key_enc: String,
    pub permission: String,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub last_error: Option<String>,
    /// Certificate trusted by an administrator (SHA-256): the only one accepted for this node.
    pub tls_fingerprint: Option<String>,
    /// Capabilities of the node (JSON, ADR 0012) and when they were received.
    pub peer_caps: Option<String>,
    pub peer_caps_at: Option<i64>,
}

pub async fn set_node_caps(db: &Db, id: i64, caps: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE nodes SET peer_caps = ?, peer_caps_at = ? WHERE id = ?").bind(caps).bind(now()).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_share_caps(db: &Db, id: i64, caps: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE shares SET peer_caps = ?, peer_caps_at = ? WHERE id = ?").bind(caps).bind(now()).bind(id).execute(db).await?;
    Ok(())
}

pub async fn list_nodes(db: &Db) -> sqlx::Result<Vec<Node>> {
    sqlx::query_as("SELECT * FROM nodes ORDER BY name").fetch_all(db).await
}

pub async fn node_by_id(db: &Db, id: i64) -> sqlx::Result<Option<Node>> {
    sqlx::query_as("SELECT * FROM nodes WHERE id = ?").bind(id).fetch_optional(db).await
}

/// Adds a node, or replaces the URL and key of a node added before (same remote id).
pub async fn upsert_node(db: &Db, remote_node_id: &str, name: &str, url: &str, key_enc: &str, permission: &str) -> sqlx::Result<Node> {
    sqlx::query_as(
        "INSERT INTO nodes (remote_node_id, name, url, key_enc, permission, created_at, last_seen) VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (remote_node_id) DO UPDATE SET name = excluded.name, url = excluded.url, key_enc = excluded.key_enc,
           permission = excluded.permission, last_seen = excluded.last_seen, last_error = NULL
         RETURNING *",
    )
    .bind(remote_node_id)
    .bind(name)
    .bind(url)
    .bind(key_enc)
    .bind(permission)
    .bind(now())
    .bind(now())
    .fetch_one(db)
    .await
}

/// Installations of a node this node chose not to show (their ids on that node).
pub async fn node_exclusions(db: &Db, node_id: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar("SELECT remote_id FROM node_excluded_installations WHERE node_id = ? ORDER BY remote_id").bind(node_id).fetch_all(db).await
}

pub async fn set_node_exclusions(db: &Db, node_id: i64, remote_ids: &[i64]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM node_excluded_installations WHERE node_id = ?").bind(node_id).execute(&mut *tx).await?;
    for id in remote_ids {
        sqlx::query("INSERT OR IGNORE INTO node_excluded_installations (node_id, remote_id) VALUES (?, ?)").bind(node_id).bind(id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn set_node_fingerprint(db: &Db, id: i64, fingerprint: Option<&str>) -> sqlx::Result<()> {
    sqlx::query("UPDATE nodes SET tls_fingerprint = ? WHERE id = ?").bind(fingerprint).bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_node_url(db: &Db, id: i64, url: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE nodes SET url = ? WHERE id = ?").bind(url).bind(id).execute(db).await?;
    sqlx::query("UPDATE installations SET host = ? WHERE node_id = ?").bind(url).bind(id).execute(db).await?;
    Ok(())
}

/// Result of the last call to a node.
pub async fn set_node_seen(db: &Db, id: i64, name: Option<&str>, permission: Option<&str>, error: Option<&str>) -> sqlx::Result<()> {
    if error.is_some() {
        sqlx::query("UPDATE nodes SET last_error = ? WHERE id = ?").bind(error).bind(id).execute(db).await?;
    } else {
        // Written at most once a minute when nothing else changes: a node is called every few seconds.
        let t = now();
        sqlx::query(
            "UPDATE nodes SET last_seen = ?, last_error = NULL, name = COALESCE(?, name), permission = COALESCE(?, permission)
             WHERE id = ? AND (last_seen IS NULL OR last_seen < ? OR last_error IS NOT NULL OR name <> COALESCE(?, name) OR permission <> COALESCE(?, permission))",
        )
        .bind(t)
        .bind(name)
        .bind(permission)
        .bind(id)
        .bind(t - 60)
        .bind(name)
        .bind(permission)
        .execute(db)
        .await?;
    }
    Ok(())
}

pub async fn delete_node(db: &Db, id: i64) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM installations WHERE node_id = ?").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM nodes WHERE id = ?").bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
