use rusqlite::Connection;
use std::{
    fs,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn new(_app: &tauri::App) -> Self {
        // Use exe directory for portable data storage
        let mut dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("woxmail-data");
        let _ = fs::create_dir_all(&dir);
        dir.push("woxmail.db");

        let conn = Connection::open(dir).expect("failed to open sqlite db");
        Self::configure(&conn);
        Self::migrate(&conn);

        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        f(&conn)
    }

    pub fn with_conn_mut<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        f(&mut conn)
    }

    fn configure(conn: &Connection) {
        conn.execute_batch(
            r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -32768;
            "#,
        )
        .expect("failed to configure sqlite db");
    }

    fn migrate(conn: &Connection) {
        conn.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY NOT NULL,
  provider TEXT NOT NULL,
  name TEXT NOT NULL,
  email TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS folders (
  id TEXT PRIMARY KEY NOT NULL,
  account_id TEXT NOT NULL,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  delimiter TEXT,
  selectable INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  UNIQUE(account_id, path)
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY NOT NULL,
  account_id TEXT NOT NULL,
  folder_path TEXT NOT NULL,
  subject TEXT NOT NULL,
  from_name TEXT NOT NULL,
  from_email TEXT NOT NULL,
  to_emails TEXT NOT NULL,
  date_ts INTEGER NOT NULL,
  snippet TEXT NOT NULL,
  body TEXT NOT NULL,
  body_fetched INTEGER NOT NULL DEFAULT 1,
  is_read INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS attachments (
  id TEXT PRIMARY KEY NOT NULL,
  message_id TEXT NOT NULL,
  filename TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  content BLOB,
  content_id TEXT,
  disposition TEXT NOT NULL,
  attachment_index INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS folder_sync_state (
  account_id TEXT NOT NULL,
  folder_path TEXT NOT NULL,
  last_uid INTEGER NOT NULL DEFAULT 0,
  uid_validity INTEGER,
  remote_state_checked_at INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(account_id, folder_path)
);

CREATE TABLE IF NOT EXISTS account_settings (
  account_id TEXT PRIMARY KEY NOT NULL,
  imap_host TEXT NOT NULL,
  imap_port INTEGER NOT NULL,
  imap_tls INTEGER NOT NULL,
  imap_username TEXT NOT NULL,
  smtp_host TEXT NOT NULL,
  smtp_port INTEGER NOT NULL,
  smtp_tls INTEGER NOT NULL,
  smtp_username TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS account_secrets (
  account_id TEXT PRIMARY KEY NOT NULL,
  encrypted_password TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS account_oauth_tokens (
  account_id TEXT PRIMARY KEY NOT NULL,
  provider TEXT NOT NULL,
  client_id TEXT NOT NULL,
  encrypted_access_token TEXT NOT NULL,
  encrypted_refresh_token TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS message_tags (
  message_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY(message_id, tag)
);

CREATE TABLE IF NOT EXISTS compose_drafts (
  scope TEXT PRIMARY KEY NOT NULL,
  account_id TEXT NOT NULL,
  to_emails TEXT NOT NULL,
  subject TEXT NOT NULL,
  body TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS outbox_jobs (
  id TEXT PRIMARY KEY NOT NULL,
  account_id TEXT NOT NULL,
  to_emails TEXT NOT NULL,
  subject TEXT NOT NULL,
  body TEXT NOT NULL,
  is_html INTEGER NOT NULL,
  sent_folder_path TEXT NOT NULL,
  status TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  next_attempt_at INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cache_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  body_retention_days INTEGER NOT NULL DEFAULT 30,
  attachment_max_mb INTEGER NOT NULL DEFAULT 500,
  total_cache_max_mb INTEGER NOT NULL DEFAULT 2000,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS contacts (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  email TEXT NOT NULL,
  phone TEXT,
  notes TEXT,
  avatar_url TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS translation_cache (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_hash TEXT NOT NULL,
  source_text TEXT NOT NULL,
  source_lang TEXT NOT NULL DEFAULT 'auto',
  target_lang TEXT NOT NULL,
  translated_text TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS filter_rules (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  field TEXT NOT NULL,
  operator TEXT NOT NULL,
  value TEXT NOT NULL,
  action_type TEXT NOT NULL,
  action_value TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
            "#,
        )
        .expect("failed to run migrations");

        let has_source_id = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'source_id' LIMIT 1")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_source_id {
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN source_id TEXT", []);
        }

        let has_imap_uid = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'imap_uid' LIMIT 1")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_imap_uid {
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN imap_uid INTEGER", []);
        }

        let has_folder_delimiter = conn
            .prepare("SELECT 1 FROM pragma_table_info('folders') WHERE name = 'delimiter' LIMIT 1")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_folder_delimiter {
            let _ = conn.execute("ALTER TABLE folders ADD COLUMN delimiter TEXT", []);
        }

        let has_folder_selectable = conn
            .prepare("SELECT 1 FROM pragma_table_info('folders') WHERE name = 'selectable' LIMIT 1")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_folder_selectable {
            let _ = conn.execute(
                "ALTER TABLE folders ADD COLUMN selectable INTEGER NOT NULL DEFAULT 1",
                [],
            );
        }

        let has_attachment_content = conn
            .prepare(
                "SELECT 1 FROM pragma_table_info('attachments') WHERE name = 'content' LIMIT 1",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_attachment_content {
            let _ = conn.execute("ALTER TABLE attachments ADD COLUMN content BLOB", []);
        }

        let has_attachment_index = conn
            .prepare(
                "SELECT 1 FROM pragma_table_info('attachments') WHERE name = 'attachment_index' LIMIT 1",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_attachment_index {
            let _ = conn.execute(
                "ALTER TABLE attachments ADD COLUMN attachment_index INTEGER",
                [],
            );
        }

        let has_message_body_fetched = conn
            .prepare(
                "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'body_fetched' LIMIT 1",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_message_body_fetched {
            let _ = conn.execute(
                "ALTER TABLE messages ADD COLUMN body_fetched INTEGER NOT NULL DEFAULT 1",
                [],
            );
        }

        let has_body_html = conn
            .prepare(
                "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'body_html' LIMIT 1",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_body_html {
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN body_html TEXT", []);
        }

        let has_message_id = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'rfc_message_id' LIMIT 1")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        if !has_message_id {
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN rfc_message_id TEXT", []);
        }

        let has_in_reply_to = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'in_reply_to' LIMIT 1")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        if !has_in_reply_to {
            let _ = conn.execute("ALTER TABLE messages ADD COLUMN in_reply_to TEXT", []);
        }

        let has_sync_uid_validity = conn
            .prepare(
                "SELECT 1 FROM pragma_table_info('folder_sync_state') WHERE name = 'uid_validity' LIMIT 1",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_sync_uid_validity {
            let _ = conn.execute(
                "ALTER TABLE folder_sync_state ADD COLUMN uid_validity INTEGER",
                [],
            );
        }

        let has_sync_remote_state_checked_at = conn
            .prepare(
                "SELECT 1 FROM pragma_table_info('folder_sync_state') WHERE name = 'remote_state_checked_at' LIMIT 1",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);

        if !has_sync_remote_state_checked_at {
            let _ = conn.execute(
                "ALTER TABLE folder_sync_state ADD COLUMN remote_state_checked_at INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_source ON messages(account_id, folder_path, source_id)",
            [],
        );

        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_imap_uid ON messages(account_id, folder_path, imap_uid)",
            [],
        );

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_folder_date ON messages(account_id, folder_path, date_ts DESC)",
            [],
        );

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_unread ON messages(is_read, account_id, folder_path)",
            [],
        );

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id)",
            [],
        );

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_tags_message ON message_tags(message_id)",
            [],
        );

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_outbox_jobs_status_next ON outbox_jobs(status, next_attempt_at, created_at)",
            [],
        );

        Self::configure_search_index(conn);

        let _ = conn.execute("PRAGMA optimize", []);
    }

    fn configure_search_index(conn: &Connection) {
        if conn
            .execute_batch(
                r#"
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
  subject,
  from_name,
  from_email,
  to_emails,
  snippet,
  body,
  content='messages',
  content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS messages_fts_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, subject, from_name, from_email, to_emails, snippet, body)
  VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.to_emails, new.snippet, new.body);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_email, to_emails, snippet, body)
  VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.to_emails, old.snippet, old.body);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_au AFTER UPDATE OF subject, from_name, from_email, to_emails, snippet, body ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_email, to_emails, snippet, body)
  VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.to_emails, old.snippet, old.body);
  INSERT INTO messages_fts(rowid, subject, from_name, from_email, to_emails, snippet, body)
  VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.to_emails, new.snippet, new.body);
END;
                "#,
            )
            .is_err()
        {
            return;
        }

        let message_count = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0);
        let index_count = conn
            .query_row("SELECT COUNT(*) FROM messages_fts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0);

        if message_count != index_count {
            let _ = conn.execute(
                "INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')",
                [],
            );
        }
    }
}

pub fn unix_ts_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
