use tauri::{Manager, State};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

#[tauri::command]
pub fn open_mail(app: tauri::AppHandle, label: String, url: String) {
    crate::window::open_or_focus(app, label, url);
}

type CmdResult<T> = Result<T, String>;

fn password_entry(account_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("woxmail", &format!("account:{account_id}")).map_err(|e| e.to_string())
}

fn get_account_password(db: &crate::db::Db, account_id: &str) -> CmdResult<String> {
    if let Ok(password) =
        password_entry(account_id).and_then(|e| e.get_password().map_err(|e| e.to_string()))
    {
        return Ok(password);
    }

    db.with_conn(|conn| {
        let encrypted: String = conn
            .query_row(
                "SELECT encrypted_password FROM account_secrets WHERE account_id = ?1",
                rusqlite::params![account_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    "该账户缺少保存的密码或授权码，请点击账户旁边的“更新登录”重新保存。".to_string()
                }
                _ => e.to_string(),
            })?;

        crate::secret::unprotect_password(&encrypted)
            .map_err(|e| format!("本地加密密码解密失败，请点击账户旁边的“更新登录”重新保存。{e}"))
    })
}

fn has_account_password(db: &crate::db::Db, account_id: &str) -> bool {
    if password_entry(account_id)
        .and_then(|e| e.get_password().map(|_| ()).map_err(|e| e.to_string()))
        .is_ok()
    {
        return true;
    }

    db.with_conn(|conn| {
        conn.query_row(
            "SELECT 1 FROM account_secrets WHERE account_id = ?1 LIMIT 1",
            rusqlite::params![account_id],
            |_| Ok(()),
        )
        .map(|_| true)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            _ => Err(e.to_string()),
        })
    })
    .unwrap_or(false)
}

fn has_account_oauth(db: &crate::db::Db, account_id: &str) -> bool {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT 1 FROM account_oauth_tokens WHERE account_id = ?1 LIMIT 1",
            rusqlite::params![account_id],
            |_| Ok(()),
        )
        .map(|_| true)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            _ => Err(e.to_string()),
        })
    })
    .unwrap_or(false)
}

fn get_account_auth(db: &crate::db::Db, account_id: &str) -> CmdResult<crate::mail::MailAuth> {
    if let Some(token) = get_valid_oauth_access_token(db, account_id)? {
        return Ok(crate::mail::MailAuth::OAuth2(token));
    }

    get_account_password(db, account_id).map(crate::mail::MailAuth::Password)
}

fn save_account_password(db: &crate::db::Db, account_id: &str, password: &str) -> CmdResult<()> {
    let _ = password_entry(account_id).and_then(|e| {
        e.set_password(password)
            .and_then(|_| e.get_password().map(|_| ()))
            .map_err(|e| e.to_string())
    });

    let encrypted = crate::secret::protect_password(password)?;
    let now = crate::db::unix_ts_now();
    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO account_secrets (account_id, encrypted_password, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id) DO UPDATE SET
               encrypted_password = excluded.encrypted_password,
               updated_at = excluded.updated_at",
            rusqlite::params![account_id, encrypted, now, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    let saved = get_account_password(db, account_id)?;
    if saved != password {
        return Err("密码或授权码保存后校验失败，请重新保存登录信息".to_string());
    }
    Ok(())
}

fn get_valid_oauth_access_token(db: &crate::db::Db, account_id: &str) -> CmdResult<Option<String>> {
    use rusqlite::params;

    let row = db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT provider, client_id, encrypted_access_token, encrypted_refresh_token, expires_at
                 FROM account_oauth_tokens
                 WHERE account_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let row = stmt.query_row(params![account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        });
        match row {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })?;

    let Some((provider, client_id, encrypted_access_token, encrypted_refresh_token, expires_at)) = row else {
        return Ok(None);
    };

    if expires_at > crate::db::unix_ts_now() + 60 {
        return crate::secret::unprotect_password(&encrypted_access_token).map(Some);
    }

    let refresh_token = crate::secret::unprotect_password(&encrypted_refresh_token)?;
    let tokens = crate::oauth::refresh_access_token(&provider, &client_id, &refresh_token)?;
    save_oauth_tokens(db, account_id, &provider, &client_id, &tokens)?;
    Ok(Some(tokens.access_token))
}

fn save_oauth_tokens(
    db: &crate::db::Db,
    account_id: &str,
    provider: &str,
    client_id: &str,
    tokens: &crate::oauth::OAuthTokenSet,
) -> CmdResult<()> {
    use rusqlite::params;

    let encrypted_access_token = crate::secret::protect_password(&tokens.access_token)?;
    let encrypted_refresh_token = crate::secret::protect_password(&tokens.refresh_token)?;
    let now = crate::db::unix_ts_now();

    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO account_oauth_tokens (
               account_id, provider, client_id, encrypted_access_token,
               encrypted_refresh_token, expires_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(account_id) DO UPDATE SET
               provider = excluded.provider,
               client_id = excluded.client_id,
               encrypted_access_token = excluded.encrypted_access_token,
               encrypted_refresh_token = excluded.encrypted_refresh_token,
               expires_at = excluded.expires_at,
               updated_at = excluded.updated_at",
            params![
                account_id,
                provider,
                client_id,
                encrypted_access_token,
                encrypted_refresh_token,
                tokens.expires_at,
                now,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn upsert_gmail_oauth_account(
    db: &crate::db::Db,
    email: &str,
    name: &str,
    client_id: &str,
    tokens: &crate::oauth::OAuthTokenSet,
) -> CmdResult<crate::models::Account> {
    use rusqlite::params;

    let now = crate::db::unix_ts_now();
    let existing = db.with_conn(|conn| {
        let row = conn.query_row(
            "SELECT id, provider, name, email FROM accounts WHERE provider = 'gmail' AND lower(email) = lower(?1)",
            params![email],
            |row| {
                Ok(crate::models::Account {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    name: row.get(2)?,
                    email: row.get(3)?,
                })
            },
        );
        match row {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })?;

    let account = if let Some(mut account) = existing {
        account.name = if name.trim().is_empty() {
            email.to_string()
        } else {
            name.to_string()
        };
        db.with_conn_mut(|conn| {
            conn.execute(
                "UPDATE accounts SET name = ?1 WHERE id = ?2",
                params![account.name, account.id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
        account
    } else {
        let id = Uuid::new_v4().to_string();
        let account = crate::models::Account {
            id: id.clone(),
            provider: "gmail".to_string(),
            name: if name.trim().is_empty() {
                email.to_string()
            } else {
                name.to_string()
            },
            email: email.to_string(),
        };
        db.with_conn_mut(|conn| {
            conn.execute(
                "INSERT INTO accounts (id, provider, name, email, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![account.id, account.provider, account.name, account.email, now],
            )
            .map_err(|e| e.to_string())?;
            for (path, label) in [("INBOX", "Inbox"), ("Sent", "Sent")] {
                conn.execute(
                    "INSERT OR IGNORE INTO folders (id, account_id, path, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![Uuid::new_v4().to_string(), id, path, label, now],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })?;
        account
    };

    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO account_settings (
                account_id, imap_host, imap_port, imap_tls, imap_username,
                smtp_host, smtp_port, smtp_tls, smtp_username,
                created_at, updated_at
            ) VALUES (
                ?1, 'imap.gmail.com', 993, 1, ?2,
                'smtp.gmail.com', 465, 1, ?2,
                ?3, ?3
            )
            ON CONFLICT(account_id) DO UPDATE SET
              imap_host = excluded.imap_host,
              imap_port = excluded.imap_port,
              imap_tls = excluded.imap_tls,
              imap_username = excluded.imap_username,
              smtp_host = excluded.smtp_host,
              smtp_port = excluded.smtp_port,
              smtp_tls = excluded.smtp_tls,
              smtp_username = excluded.smtp_username,
              updated_at = excluded.updated_at",
            params![account.id, email, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    save_oauth_tokens(db, &account.id, "gmail", client_id, tokens)?;
    Ok(account)
}

fn upsert_outlook_oauth_account(
    db: &crate::db::Db,
    email: &str,
    name: &str,
    client_id: &str,
    tokens: &crate::oauth::OAuthTokenSet,
) -> CmdResult<crate::models::Account> {
    use rusqlite::params;

    let now = crate::db::unix_ts_now();
    let existing = db.with_conn(|conn| {
        let row = conn.query_row(
            "SELECT id, provider, name, email FROM accounts WHERE provider = 'outlook' AND lower(email) = lower(?1)",
            params![email],
            |row| {
                Ok(crate::models::Account {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    name: row.get(2)?,
                    email: row.get(3)?,
                })
            },
        );
        match row {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })?;

    let account = if let Some(mut account) = existing {
        account.name = if name.trim().is_empty() {
            email.to_string()
        } else {
            name.to_string()
        };
        db.with_conn_mut(|conn| {
            conn.execute(
                "UPDATE accounts SET name = ?1 WHERE id = ?2",
                params![account.name, account.id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })?;
        account
    } else {
        let id = Uuid::new_v4().to_string();
        let account = crate::models::Account {
            id: id.clone(),
            provider: "outlook".to_string(),
            name: if name.trim().is_empty() {
                email.to_string()
            } else {
                name.to_string()
            },
            email: email.to_string(),
        };
        db.with_conn_mut(|conn| {
            conn.execute(
                "INSERT INTO accounts (id, provider, name, email, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![account.id, account.provider, account.name, account.email, now],
            )
            .map_err(|e| e.to_string())?;
            for (path, label) in [("INBOX", "Inbox"), ("Sent", "Sent")] {
                conn.execute(
                    "INSERT OR IGNORE INTO folders (id, account_id, path, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![Uuid::new_v4().to_string(), id, path, label, now],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })?;
        account
    };

    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO account_settings (
                account_id, imap_host, imap_port, imap_tls, imap_username,
                smtp_host, smtp_port, smtp_tls, smtp_username,
                created_at, updated_at
            ) VALUES (
                ?1, 'outlook.office365.com', 993, 1, ?2,
                'smtp.office365.com', 587, 1, ?2,
                ?3, ?3
            )
            ON CONFLICT(account_id) DO UPDATE SET
              imap_host = excluded.imap_host,
              imap_port = excluded.imap_port,
              imap_tls = excluded.imap_tls,
              imap_username = excluded.imap_username,
              smtp_host = excluded.smtp_host,
              smtp_port = excluded.smtp_port,
              smtp_tls = excluded.smtp_tls,
              smtp_username = excluded.smtp_username,
              updated_at = excluded.updated_at",
            params![account.id, email, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    save_oauth_tokens(db, &account.id, "outlook", client_id, tokens)?;
    Ok(account)
}

#[tauri::command]
pub fn list_accounts(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<Vec<crate::models::Account>> {
    use rusqlite::params;

    state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT id, provider, name, email FROM accounts ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![], |row| {
                Ok(crate::models::Account {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    name: row.get(2)?,
                    email: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

#[tauri::command]
pub fn create_account(
    state: State<'_, crate::state::AppState>,
    input: crate::models::CreateAccountInput,
) -> CmdResult<crate::models::Account> {
    use rusqlite::params;

    let id = Uuid::new_v4().to_string();
    let now = crate::db::unix_ts_now();

    let name = if input.name.trim().is_empty() {
        input.email.clone()
    } else {
        input.name
    };

    let account = crate::models::Account {
        id: id.clone(),
        provider: input.provider,
        name,
        email: input.email,
    };

    state.db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO accounts (id, provider, name, email, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account.id, account.provider, account.name, account.email, now],
        )
        .map_err(|e| e.to_string())?;

        let inbox_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO folders (id, account_id, path, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![inbox_id, id, "INBOX", "Inbox", now],
        )
        .map_err(|e| e.to_string())?;

        let sent_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO folders (id, account_id, path, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sent_id, id, "Sent", "Sent", now],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })?;

    Ok(account)
}

#[tauri::command]
pub async fn gmail_oauth_login(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    input: crate::models::GmailOAuthLoginInput,
) -> CmdResult<crate::models::Account> {
    let client_id = input
        .client_id
        .or_else(|| std::env::var("WOXMAIL_GMAIL_OAUTH_CLIENT_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::oauth::DEFAULT_GMAIL_CLIENT_ID.to_string());

    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = crate::oauth::run_gmail_oauth(&client_id, |url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })?;
        upsert_gmail_oauth_account(&db, &result.email, &result.name, &client_id, &result.tokens)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn outlook_oauth_login(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    input: crate::models::OutlookOAuthLoginInput,
) -> CmdResult<crate::models::Account> {
    let client_id = input
        .client_id
        .or_else(|| std::env::var("WOXMAIL_OUTLOOK_OAUTH_CLIENT_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::oauth::DEFAULT_OUTLOOK_CLIENT_ID.to_string());

    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = crate::oauth::run_outlook_oauth(&client_id, |url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })?;
        upsert_outlook_oauth_account(&db, &result.email, &result.name, &client_id, &result.tokens)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn delete_account(
    state: State<'_, crate::state::AppState>,
    account_id: String,
) -> CmdResult<()> {
    use rusqlite::params;

    let _ =
        password_entry(&account_id).and_then(|e| e.delete_credential().map_err(|e| e.to_string()));

    state.db.with_conn_mut(|conn| {
        conn.execute(
            "DELETE FROM message_tags WHERE message_id IN (SELECT id FROM messages WHERE account_id = ?1)",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM attachments WHERE message_id IN (SELECT id FROM messages WHERE account_id = ?1)",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM messages WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM folder_sync_state WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM account_settings WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM account_secrets WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM account_oauth_tokens WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM folders WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![account_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn get_account_settings(
    state: State<'_, crate::state::AppState>,
    account_id: String,
) -> CmdResult<Option<crate::models::AccountSettings>> {
    get_account_settings_from_db(&state.db, &account_id)
}

fn get_account_settings_from_db(
    db: &crate::db::Db,
    account_id: &str,
) -> CmdResult<Option<crate::models::AccountSettings>> {
    use rusqlite::params;

    let has_password = has_account_password(db, account_id) || has_account_oauth(db, account_id);

    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT s.account_id, a.provider, s.imap_host, s.imap_port, s.imap_tls, s.imap_username,
                        s.smtp_host, s.smtp_port, s.smtp_tls, s.smtp_username
                 FROM account_settings s
                 JOIN accounts a ON a.id = s.account_id
                 WHERE s.account_id = ?1",
            )
            .map_err(|e| e.to_string())?;

        let row = stmt.query_row(params![account_id], |row| {
            let imap_tls: i64 = row.get(4)?;
            let smtp_tls: i64 = row.get(8)?;
            Ok(crate::models::AccountSettings {
                account_id: row.get(0)?,
                provider: row.get(1)?,
                imap_host: row.get(2)?,
                imap_port: row.get(3)?,
                imap_tls: imap_tls != 0,
                imap_username: row.get(5)?,
                smtp_host: row.get(6)?,
                smtp_port: row.get(7)?,
                smtp_tls: smtp_tls != 0,
                smtp_username: row.get(9)?,
                has_password,
            })
        });

        match row {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
}

#[tauri::command]
pub fn set_account_settings(
    state: State<'_, crate::state::AppState>,
    input: crate::models::SetAccountSettingsInput,
) -> CmdResult<()> {
    use rusqlite::params;

    if input.password.trim().is_empty() {
        return Err("password is empty".to_string());
    }

    let now = crate::db::unix_ts_now();
    save_account_password(&state.db, &input.account_id, &input.password)?;

    state.db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO account_settings (
                account_id, imap_host, imap_port, imap_tls, imap_username,
                smtp_host, smtp_port, smtp_tls, smtp_username,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11
            )
            ON CONFLICT(account_id) DO UPDATE SET
              imap_host = excluded.imap_host,
              imap_port = excluded.imap_port,
              imap_tls = excluded.imap_tls,
              imap_username = excluded.imap_username,
              smtp_host = excluded.smtp_host,
              smtp_port = excluded.smtp_port,
              smtp_tls = excluded.smtp_tls,
              smtp_username = excluded.smtp_username,
              updated_at = excluded.updated_at",
            params![
                input.account_id,
                input.imap_host,
                input.imap_port,
                if input.imap_tls { 1i64 } else { 0i64 },
                input.imap_username,
                input.smtp_host,
                input.smtp_port,
                if input.smtp_tls { 1i64 } else { 0i64 },
                input.smtp_username,
                now,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })
}

#[tauri::command]
pub async fn list_folders(
    state: State<'_, crate::state::AppState>,
    account_id: String,
) -> CmdResult<Vec<crate::models::MailFolder>> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || list_folders_blocking(db, account_id))
        .await
        .map_err(|e| e.to_string())?
}

fn list_folders_blocking(
    db: crate::db::Db,
    account_id: String,
) -> CmdResult<Vec<crate::models::MailFolder>> {
    use rusqlite::params;

    if let Some(settings) = get_account_settings_from_db(&db, &account_id)? {
        let auth = get_account_auth(&db, &account_id)?;
        let remote_folders = crate::mail::list_imap_folders(&settings, &auth)?;
        let now = crate::db::unix_ts_now();

        db.with_conn_mut(|conn| {
            conn.execute(
                "DELETE FROM folders WHERE account_id = ?1",
                params![account_id],
            )
            .map_err(|e| e.to_string())?;

            for folder in remote_folders {
                conn.execute(
                    "INSERT INTO folders (id, account_id, path, name, delimiter, selectable, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        Uuid::new_v4().to_string(),
                        account_id,
                        folder.path,
                        folder.name,
                        folder.delimiter,
                        if folder.selectable { 1i64 } else { 0i64 },
                        now
                    ],
                )
                .map_err(|e| e.to_string())?;
            }

            Ok(())
        })?;
    }

    read_folders(&db, &account_id)
}

fn read_folders(db: &crate::db::Db, account_id: &str) -> CmdResult<Vec<crate::models::MailFolder>> {
    use rusqlite::params;

    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, path, name, delimiter, selectable
                 FROM folders
                 WHERE account_id = ?1
                 ORDER BY lower(name), lower(path)",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![account_id], |row| {
                let selectable: i64 = row.get(5)?;
                Ok(crate::models::MailFolder {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    path: row.get(2)?,
                    name: row.get(3)?,
                    delimiter: row.get(4)?,
                    selectable: selectable != 0,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

fn read_message_tags(conn: &rusqlite::Connection, message_id: &str) -> CmdResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT tag FROM message_tags WHERE message_id = ?1 ORDER BY lower(tag)")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![message_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row.map_err(|e| e.to_string())?);
    }
    Ok(tags)
}

#[tauri::command]
pub async fn sync_inbox(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    account_id: String,
) -> CmdResult<usize> {
    sync_folder(app, state, account_id, "INBOX".to_string(), Some(true)).await
}

#[tauri::command]
pub async fn sync_inboxes(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
) -> CmdResult<usize> {
    let db = state.db.clone();
    let inserted_by_account = tauri::async_runtime::spawn_blocking(move || {
        let targets = inbox_sync_targets(&db)?;
        let mut results = Vec::new();

        for (account_id, folder_path) in targets {
            match sync_folder_blocking(db.clone(), account_id.clone(), folder_path) {
                Ok(inserted) => {
                    if inserted > 0 {
                        results.push((account_id, inserted));
                    }
                }
                Err(_) => {
                    // Background sync should not make one failing account block the rest.
                }
            }
        }

        Ok::<_, String>(results)
    })
    .await
    .map_err(|e| e.to_string())??;

    let total = inserted_by_account
        .iter()
        .map(|(_, inserted)| *inserted)
        .sum::<usize>();

    for (account_id, inserted) in inserted_by_account {
        show_new_mail_notification(&app, &state.db, &account_id, inserted);
    }

    Ok(total)
}

#[tauri::command]
pub async fn sync_folder(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    account_id: String,
    folder_path: String,
    notify: Option<bool>,
) -> CmdResult<usize> {
    let db = state.db.clone();
    let notification_db = state.db.clone();
    let notification_account_id = account_id.clone();
    let should_notify = notify.unwrap_or(true);
    let inserted = tauri::async_runtime::spawn_blocking(move || {
        sync_folder_blocking(db, account_id, folder_path)
    })
    .await
    .map_err(|e| e.to_string())??;

    if should_notify && inserted > 0 {
        show_new_mail_notification(&app, &notification_db, &notification_account_id, inserted);
    }

    Ok(inserted)
}

#[tauri::command]
pub async fn sync_folder_deep(
    state: State<'_, crate::state::AppState>,
    account_id: String,
    folder_path: String,
) -> CmdResult<usize> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_folder_blocking_with_limit(db, account_id, folder_path, 500)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sync_folder_blocking(
    db: crate::db::Db,
    account_id: String,
    folder_path: String,
) -> CmdResult<usize> {
    sync_folder_blocking_with_limit(db, account_id, folder_path, 50)
}

fn sync_folder_blocking_with_limit(
    db: crate::db::Db,
    account_id: String,
    folder_path: String,
    limit: usize,
) -> CmdResult<usize> {
    if let Some(settings) = get_account_settings_from_db(&db, &account_id)? {
        let auth = get_account_auth(&db, &account_id)?;

        return crate::mail::sync_imap_folder(&db, &account_id, &settings, &auth, &folder_path, limit);
    }

    Ok(0)
}

fn inbox_sync_targets(db: &crate::db::Db) -> CmdResult<Vec<(String, String)>> {
    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT
                   a.id,
                   COALESCE(
                     (
                       SELECT f.path
                       FROM folders f
                       WHERE f.account_id = a.id
                         AND f.selectable = 1
                         AND (
                           lower(f.path) = 'inbox'
                           OR lower(f.name) LIKE '%inbox%'
                           OR f.name LIKE '%收件%'
                         )
                       ORDER BY lower(f.path)
                       LIMIT 1
                     ),
                     (
                       SELECT f.path
                       FROM folders f
                       WHERE f.account_id = a.id AND f.selectable = 1
                       ORDER BY lower(f.name), lower(f.path)
                       LIMIT 1
                     ),
                     'INBOX'
                   )
                 FROM accounts a
                 ORDER BY a.created_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut targets = Vec::new();
        for row in rows {
            targets.push(row.map_err(|e| e.to_string())?);
        }

        Ok(targets)
    })
}

fn show_new_mail_notification(
    app: &tauri::AppHandle,
    db: &crate::db::Db,
    account_id: &str,
    inserted: usize,
) {
    let email = db
        .with_conn(|conn| {
            conn.query_row(
                "SELECT email FROM accounts WHERE id = ?1",
                rusqlite::params![account_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())
        })
        .unwrap_or_else(|_| "邮箱".to_string());

    let body = if inserted == 1 {
        format!("{email} 收到 1 封新邮件")
    } else {
        format!("{email} 收到 {inserted} 封新邮件")
    };

    let _ = app
        .notification()
        .builder()
        .title("Wox Mail")
        .body(body)
        .show();
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, crate::state::AppState>,
    account_id: String,
    folder_path: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> CmdResult<Vec<crate::models::MessageSummary>> {
    use rusqlite::params;

    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);

    state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT
                   m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
                   m.to_emails, m.date_ts, m.snippet, m.is_read, COUNT(a.id) AS attachment_count
                 FROM messages m
                 LEFT JOIN attachments a ON a.message_id = m.id
                 WHERE m.account_id = ?1 AND m.folder_path = ?2
                 GROUP BY m.id
                 ORDER BY m.date_ts DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![account_id, folder_path, limit, offset], |row| {
                let to_emails_json: String = row.get(6)?;
                let to_emails: Vec<String> =
                    serde_json::from_str(&to_emails_json).unwrap_or_default();
                let is_read: i64 = row.get(9)?;
                Ok(crate::models::MessageSummary {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    folder_path: row.get(2)?,
                    subject: row.get(3)?,
                    from_name: row.get(4)?,
                    from_email: row.get(5)?,
                    to_emails,
                    date_ts: row.get(7)?,
                    snippet: row.get(8)?,
                    is_read: is_read != 0,
                    attachment_count: row.get(10)?,
                    tags: Vec::new(),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            let mut message = row.map_err(|e| e.to_string())?;
            message.tags = read_message_tags(conn, &message.id)?;
            out.push(message);
        }
        Ok(out)
    })
}

#[tauri::command]
pub fn search_messages(
    state: State<'_, crate::state::AppState>,
    query: String,
    account_id: Option<String>,
    limit: Option<i64>,
) -> CmdResult<Vec<crate::models::MessageSummary>> {
    use rusqlite::params;

    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.unwrap_or(100).clamp(1, 300);
    let account_filter = account_id
        .filter(|value| value != "all")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let pattern = format!("%{}%", query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));

    state.db.with_conn(|conn| {
        let sql = if account_filter.is_some() {
            "SELECT
               m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
               m.to_emails, m.date_ts, m.snippet, m.is_read, COUNT(a.id) AS attachment_count
             FROM messages m
             LEFT JOIN attachments a ON a.message_id = m.id
             WHERE m.account_id = ?1
               AND (
                 m.subject LIKE ?2 ESCAPE '\\'
                 OR m.from_name LIKE ?2 ESCAPE '\\'
                 OR m.from_email LIKE ?2 ESCAPE '\\'
                 OR m.to_emails LIKE ?2 ESCAPE '\\'
                 OR m.snippet LIKE ?2 ESCAPE '\\'
                 OR m.body LIKE ?2 ESCAPE '\\'
               )
             GROUP BY m.id
             ORDER BY m.date_ts DESC
             LIMIT ?3"
        } else {
            "SELECT
               m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
               m.to_emails, m.date_ts, m.snippet, m.is_read, COUNT(a.id) AS attachment_count
             FROM messages m
             LEFT JOIN attachments a ON a.message_id = m.id
             WHERE
               m.subject LIKE ?1 ESCAPE '\\'
               OR m.from_name LIKE ?1 ESCAPE '\\'
               OR m.from_email LIKE ?1 ESCAPE '\\'
               OR m.to_emails LIKE ?1 ESCAPE '\\'
               OR m.snippet LIKE ?1 ESCAPE '\\'
               OR m.body LIKE ?1 ESCAPE '\\'
             GROUP BY m.id
             ORDER BY m.date_ts DESC
             LIMIT ?2"
        };

        let map_row = |row: &rusqlite::Row<'_>| {
            let to_emails_json: String = row.get(6)?;
            let to_emails: Vec<String> = serde_json::from_str(&to_emails_json).unwrap_or_default();
            let is_read: i64 = row.get(9)?;
            Ok(crate::models::MessageSummary {
                id: row.get(0)?,
                account_id: row.get(1)?,
                folder_path: row.get(2)?,
                subject: row.get(3)?,
                from_name: row.get(4)?,
                from_email: row.get(5)?,
                to_emails,
                date_ts: row.get(7)?,
                snippet: row.get(8)?,
                is_read: is_read != 0,
                attachment_count: row.get(10)?,
                tags: Vec::new(),
            })
        };

        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = if let Some(account_id) = account_filter {
            stmt.query_map(params![account_id, pattern, limit], map_row)
                .map_err(|e| e.to_string())?
        } else {
            stmt.query_map(params![pattern, limit], map_row)
                .map_err(|e| e.to_string())?
        };

        let mut out = Vec::new();
        for row in rows {
            let mut message = row.map_err(|e| e.to_string())?;
            message.tags = read_message_tags(conn, &message.id)?;
            out.push(message);
        }
        Ok(out)
    })
}

#[tauri::command]
pub fn get_message(
    state: State<'_, crate::state::AppState>,
    message_id: String,
) -> CmdResult<crate::models::MessageDetail> {
    use rusqlite::params;

    state.db.with_conn(|conn| {
        let mut message = conn.query_row(
            "SELECT id, account_id, folder_path, subject, from_name, from_email, to_emails, date_ts, body, is_read
             FROM messages
             WHERE id = ?1",
             params![message_id],
             |row| {
                 let to_emails_json: String = row.get(6)?;
                 let to_emails: Vec<String> = serde_json::from_str(&to_emails_json).unwrap_or_default();
                 let is_read: i64 = row.get(9)?;

                 Ok(crate::models::MessageDetail {
                     id: row.get(0)?,
                     account_id: row.get(1)?,
                     folder_path: row.get(2)?,
                     subject: row.get(3)?,
                     from_name: row.get(4)?,
                     from_email: row.get(5)?,
                     to_emails,
                     date_ts: row.get(7)?,
                     body: row.get(8)?,
                     is_read: is_read != 0,
                     attachments: Vec::new(),
                     tags: Vec::new(),
                 })
             },
        )
        .map_err(|e| e.to_string())?;

        message.tags = read_message_tags(conn, &message.id)?;

        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, filename, mime_type, size_bytes, content_id, disposition
                 FROM attachments
                 WHERE message_id = ?1
                 ORDER BY filename ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![message.id.as_str()], |row| {
                Ok(crate::models::MessageAttachment {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    filename: row.get(2)?,
                    mime_type: row.get(3)?,
                    size_bytes: row.get(4)?,
                    content_id: row.get(5)?,
                    disposition: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        for row in rows {
            message.attachments.push(row.map_err(|e| e.to_string())?);
        }

        Ok(message)
    })
}

#[tauri::command]
pub fn save_attachment(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    attachment_id: String,
) -> CmdResult<String> {
    let attachment = read_attachment_content(&state.db, &attachment_id)?;
    let mut dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| e.to_string())?;
    dir.push("Wox Mail Attachments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let filename = safe_attachment_filename(&attachment.filename);
    let path = unique_attachment_path(dir, &filename);
    std::fs::write(&path, attachment.content).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn open_attachment(
    app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    attachment_id: String,
) -> CmdResult<String> {
    let path = save_attachment(app.clone(), state, attachment_id)?;
    app.opener()
        .open_path(path.clone(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
pub fn list_unread_counts(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<Vec<crate::models::UnreadCount>> {
    state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT account_id, folder_path, COUNT(*)
                 FROM messages
                 WHERE is_read = 0
                 GROUP BY account_id, folder_path",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::UnreadCount {
                    account_id: row.get(0)?,
                    folder_path: row.get(1)?,
                    unread_count: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

#[tauri::command]
pub fn get_compose_draft(
    state: State<'_, crate::state::AppState>,
    scope: String,
) -> CmdResult<Option<crate::models::ComposeDraft>> {
    state.db.with_conn(|conn| {
        let row = conn.query_row(
            "SELECT scope, account_id, to_emails, subject, body, updated_at
             FROM compose_drafts
             WHERE scope = ?1",
            rusqlite::params![scope],
            |row| {
                let to_emails_json: String = row.get(2)?;
                let to_emails = serde_json::from_str(&to_emails_json).unwrap_or_default();
                Ok(crate::models::ComposeDraft {
                    scope: row.get(0)?,
                    account_id: row.get(1)?,
                    to_emails,
                    subject: row.get(3)?,
                    body: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        );
        match row {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
}

#[tauri::command]
pub fn save_compose_draft(
    state: State<'_, crate::state::AppState>,
    input: crate::models::SaveComposeDraftInput,
) -> CmdResult<()> {
    let scope = input.scope.trim().to_string();
    if scope.is_empty() {
        return Err("草稿 scope 不能为空".to_string());
    }

    let now = crate::db::unix_ts_now();
    let to_emails = serde_json::to_string(&input.to_emails).map_err(|e| e.to_string())?;
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO compose_drafts (scope, account_id, to_emails, subject, body, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(scope) DO UPDATE SET
               account_id = excluded.account_id,
               to_emails = excluded.to_emails,
               subject = excluded.subject,
               body = excluded.body,
               updated_at = excluded.updated_at",
            rusqlite::params![
                scope,
                input.account_id,
                to_emails,
                input.subject,
                input.body,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn delete_compose_draft(
    state: State<'_, crate::state::AppState>,
    scope: String,
) -> CmdResult<()> {
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "DELETE FROM compose_drafts WHERE scope = ?1",
            rusqlite::params![scope],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

struct AttachmentContent {
    filename: String,
    content: Vec<u8>,
}

fn read_attachment_content(
    db: &crate::db::Db,
    attachment_id: &str,
) -> CmdResult<AttachmentContent> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT filename, content FROM attachments WHERE id = ?1",
            rusqlite::params![attachment_id],
            |row| {
                Ok(AttachmentContent {
                    filename: row.get(0)?,
                    content: row.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default(),
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "附件不存在".to_string(),
            _ => e.to_string(),
        })
        .and_then(|attachment| {
            if attachment.content.is_empty() {
                Err("该附件没有本地内容。请刷新或重新同步这封邮件后再试。".to_string())
            } else {
                Ok(attachment)
            }
        })
    })
}

fn safe_attachment_filename(filename: &str) -> String {
    let cleaned = filename
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned
    }
}

fn unique_attachment_path(mut dir: std::path::PathBuf, filename: &str) -> std::path::PathBuf {
    dir.push(filename);
    if !dir.exists() {
        return dir;
    }

    let original = dir.clone();
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let ext = original.extension().and_then(|value| value.to_str());
    let parent = original.parent().map(std::path::Path::to_path_buf).unwrap_or_default();

    for index in 1..10_000 {
        let candidate_name = if let Some(ext) = ext {
            format!("{stem} ({index}).{ext}")
        } else {
            format!("{stem} ({index})")
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    original
}

#[tauri::command]
pub fn add_message_tag(
    state: State<'_, crate::state::AppState>,
    message_ids: Vec<String>,
    tag: String,
) -> CmdResult<()> {
    use rusqlite::params;

    let tag = tag.trim().to_string();
    if tag.is_empty() {
        return Err("标签不能为空".to_string());
    }
    let now = crate::db::unix_ts_now();

    state.db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for message_id in message_ids {
            tx.execute(
                "INSERT OR IGNORE INTO message_tags (message_id, tag, created_at)
                 VALUES (?1, ?2, ?3)",
                params![message_id, tag, now],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn clear_message_tags(
    state: State<'_, crate::state::AppState>,
    message_ids: Vec<String>,
) -> CmdResult<()> {
    use rusqlite::params;

    state.db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for message_id in message_ids {
            tx.execute(
                "DELETE FROM message_tags WHERE message_id = ?1",
                params![message_id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn move_messages_to_folder(
    state: State<'_, crate::state::AppState>,
    message_ids: Vec<String>,
    folder_path: String,
) -> CmdResult<()> {
    use rusqlite::params;

    if folder_path.trim().is_empty() {
        return Err("目标文件夹不能为空".to_string());
    }

    let target_folder = folder_path.trim().to_string();
    let mut groups = state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, folder_path, imap_uid
                 FROM messages
                 WHERE id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut groups =
            std::collections::BTreeMap::<(String, String), Vec<(String, Option<i64>)>>::new();

        for message_id in &message_ids {
            let row = stmt.query_row(params![message_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            });
            match row {
                Ok((id, account_id, source_folder, imap_uid)) => {
                    groups
                        .entry((account_id, source_folder))
                        .or_default()
                        .push((id, imap_uid));
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => return Err(e.to_string()),
            }
        }

        Ok(groups)
    })?;

    for ((account_id, source_folder), messages) in &mut groups {
        if source_folder == &target_folder {
            continue;
        }

        let imap_uids = messages
            .iter()
            .filter_map(|(_, uid)| *uid)
            .collect::<Vec<_>>();
        if imap_uids.is_empty() {
            continue;
        }

        let Some(settings) = get_account_settings_from_db(&state.db, account_id)? else {
            return Err("该账户缺少服务器登录设置，无法同步移动邮件".to_string());
        };
        let auth = get_account_auth(&state.db, account_id)?;
        crate::mail::move_imap_messages(
            &settings,
            &auth,
            source_folder,
            &target_folder,
            &imap_uids,
        )?;
    }

    state.db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for message_id in message_ids {
            tx.execute(
                "UPDATE messages SET folder_path = ?1 WHERE id = ?2",
                params![target_folder, message_id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn mark_message_read(
    state: State<'_, crate::state::AppState>,
    message_id: String,
) -> CmdResult<()> {
    use rusqlite::params;

    let (account_id, folder_path, imap_uid, was_read): (String, String, Option<i64>, i64) =
        state.db.with_conn(|conn| {
            conn.query_row(
                "SELECT account_id, folder_path, imap_uid, is_read FROM messages WHERE id = ?1",
                params![message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| e.to_string())
        })?;

    if was_read != 0 {
        return Ok(());
    }

    if let Some(uid) = imap_uid {
        if let Some(settings) = get_account_settings(state.clone(), account_id.clone())? {
            let auth = get_account_auth(&state.db, &account_id)?;
            crate::mail::mark_imap_message_seen(&settings, &auth, &folder_path, uid)?;
        }
    }

    state.db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE messages SET is_read = 1 WHERE id = ?1",
            params![message_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn send_message(
    state: State<'_, crate::state::AppState>,
    input: crate::models::SendMessageInput,
) -> CmdResult<String> {
    use rusqlite::params;

    let now = crate::db::unix_ts_now();
    let msg_id = Uuid::new_v4().to_string();
    let is_html = input.is_html.unwrap_or(false);
    let to_emails_json =
        serde_json::to_string(&input.to_emails).unwrap_or_else(|_| "[]".to_string());

    let (from_name, from_email): (String, String) = state.db.with_conn(|conn| {
        conn.query_row(
            "SELECT name, email FROM accounts WHERE id = ?1",
            params![input.account_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())
    })?;

    if let Some(settings) = get_account_settings(state.clone(), input.account_id.clone())? {
        let auth = get_account_auth(&state.db, &input.account_id)?;

        crate::mail::send_smtp(
            &settings,
            &auth,
            &from_email,
            &from_name,
            &input.to_emails,
            &input.subject,
            &input.body,
            is_html,
        )?;
    }

    state.db.with_conn_mut(|conn| {
        let snippet_source = if is_html {
            html_to_textish(&input.body)
        } else {
            input.body.clone()
        };
        let snippet = snippet_source.lines().next().unwrap_or("").to_string();
        let sent_folder_path = input
            .sent_folder_path
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Sent");

        conn.execute(
            "INSERT INTO messages (id, account_id, folder_path, subject, from_name, from_email, to_emails, date_ts, snippet, body, is_read, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)",
            params![
                msg_id,
                input.account_id,
                sent_folder_path,
                input.subject,
                from_name,
                from_email,
                to_emails_json,
                now,
                snippet,
                input.body,
                now
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    })?;

    Ok(msg_id)
}

fn html_to_textish(value: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
