use rusqlite::Connection;
use std::{
    fs,
    sync::{Arc, Mutex},
};
use tauri::Manager;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn new(app: &tauri::App) -> Self {
        let mut dir = app
            .path()
            .app_data_dir()
            .expect("failed to resolve app data dir");
        let _ = fs::create_dir_all(&dir);
        dir.push("woxmail.db");

        let conn = Connection::open(dir).expect("failed to open sqlite db");
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
  is_read INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS attachments (
  id TEXT PRIMARY KEY NOT NULL,
  message_id TEXT NOT NULL,
  filename TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  content_id TEXT,
  disposition TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS folder_sync_state (
  account_id TEXT NOT NULL,
  folder_path TEXT NOT NULL,
  last_uid INTEGER NOT NULL DEFAULT 0,
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
            "CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id)",
            [],
        );

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_message_tags_message ON message_tags(message_id)",
            [],
        );
    }
}

pub fn unix_ts_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
