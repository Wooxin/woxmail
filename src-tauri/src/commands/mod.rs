use tauri::State;
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

    let Some((provider, client_id, encrypted_access_token, encrypted_refresh_token, expires_at)) =
        row
    else {
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

        // Limit concurrent sync to 2 accounts at a time
        let max_concurrent = 2usize;
        for chunk in targets.chunks(max_concurrent) {
            for (account_id, folder_path) in chunk {
                match sync_folder_blocking(db.clone(), account_id.clone(), folder_path.clone()) {
                    Ok(inserted) => {
                        if inserted > 0 {
                            results.push((account_id.clone(), inserted));
                        }
                    }
                    Err(_) => {
                        // Background sync should not make one failing account block the rest.
                    }
                }
            }
        }

        // Apply filter rules to newly synced messages
        for (account_id, _) in &results {
            let _ = apply_filters_to_new_messages(&db, account_id);
        }

        // Auto-purge old messages based on cache settings
        if let Ok(settings) = db.with_conn(|conn| {
            conn.query_row(
                "SELECT body_retention_days FROM cache_settings WHERE id = 1",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())
        }) {
            let cutoff = crate::db::unix_ts_now() - settings * 86400;
            let _ = db.with_conn_mut(|conn| {
                conn.execute(
                    "DELETE FROM messages WHERE date_ts < ?1 AND body_fetched = 1",
                    rusqlite::params![cutoff],
                )
                .map_err(|e| e.to_string())
                .map(|_| ())
            });
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
        sync_folder_blocking_with_limit(db, account_id, folder_path, 500, true)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sync_folder_blocking(
    db: crate::db::Db,
    account_id: String,
    folder_path: String,
) -> CmdResult<usize> {
    sync_folder_blocking_with_limit(db, account_id, folder_path, 50, false)
}

fn sync_folder_blocking_with_limit(
    db: crate::db::Db,
    account_id: String,
    folder_path: String,
    limit: usize,
    force_remote_state_check: bool,
) -> CmdResult<usize> {
    if let Some(settings) = get_account_settings_from_db(&db, &account_id)? {
        let auth = get_account_auth(&db, &account_id)?;

        return crate::mail::sync_imap_folder(
            &db,
            &account_id,
            &settings,
            &auth,
            &folder_path,
            limit,
            force_remote_state_check,
        );
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

    // Get latest subject for preview
    let subject_preview: String = db.with_conn(|conn| {
        conn.query_row(
            "SELECT subject FROM messages WHERE account_id = ?1 ORDER BY date_ts DESC LIMIT 1",
            rusqlite::params![account_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())
    }).unwrap_or_default();

    let body = if inserted == 1 {
        if !subject_preview.is_empty() {
            subject_preview.clone()
        } else {
            format!("{email} 收到 1 封新邮件")
        }
    } else {
        format!("{email} 收到 {inserted} 封新邮件")
    };

    let _ = app
        .notification()
        .builder()
        .title("Wox Mail")
        .body(body)
        .show();

    // Bring window to front
    crate::window::show_main(app);
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, crate::state::AppState>,
    account_id: String,
    folder_path: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> CmdResult<Vec<crate::models::MessageSummary>> {

    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    let is_all_accounts = account_id == "all" || account_id.is_empty();

    state.db.with_conn(|conn| {
        let sql = if is_all_accounts {
            "SELECT
               m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
               m.to_emails, m.date_ts, m.snippet, m.is_read, COUNT(a.id) AS attachment_count
             FROM messages m
             LEFT JOIN attachments a ON a.message_id = m.id
             WHERE m.folder_path = ?1
             GROUP BY m.id
             ORDER BY m.date_ts DESC
             LIMIT ?2 OFFSET ?3"
        } else {
            "SELECT
               m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
               m.to_emails, m.date_ts, m.snippet, m.is_read, COUNT(a.id) AS attachment_count
             FROM messages m
             LEFT JOIN attachments a ON a.message_id = m.id
             WHERE m.account_id = ?1 AND m.folder_path = ?2
             GROUP BY m.id
             ORDER BY m.date_ts DESC
             LIMIT ?3 OFFSET ?4"
        };

        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let params: &[&dyn rusqlite::types::ToSql] = if is_all_accounts {
            &[&folder_path, &limit, &offset]
        } else {
            &[&account_id, &folder_path, &limit, &offset]
        };
        let rows = stmt
            .query_map(params, |row| message_summary_from_row(row))
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
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.unwrap_or(100).clamp(1, 300);
    let account_filter = account_id
        .filter(|value| value != "all")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    state.db.with_conn(|conn| {
        if let Some(fts_query) = build_fts_query(&query) {
            if messages_fts_available(conn) {
                match search_messages_fts(conn, &fts_query, account_filter.as_deref(), limit) {
                    Ok(out) => return Ok(out),
                    Err(_) => {}
                }
            }
        }

        search_messages_like(conn, &query, account_filter.as_deref(), limit)
    })
}

fn build_fts_query(query: &str) -> Option<String> {
    if query
        .chars()
        .any(|ch| !ch.is_ascii() && !ch.is_whitespace())
    {
        return None;
    }

    let tokens = query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(8)
        .map(|value| format!("{value}*"))
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}

fn messages_fts_available(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'messages_fts' LIMIT 1",
        [],
        |_| Ok(()),
    )
    .is_ok()
}

fn search_messages_fts(
    conn: &rusqlite::Connection,
    fts_query: &str,
    account_filter: Option<&str>,
    limit: i64,
) -> CmdResult<Vec<crate::models::MessageSummary>> {
    use rusqlite::params;

    let sql = if account_filter.is_some() {
        "SELECT
           m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
           m.to_emails, m.date_ts, m.snippet, m.is_read,
           (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS attachment_count
         FROM messages m
         JOIN messages_fts ON messages_fts.rowid = m.rowid
         WHERE messages_fts MATCH ?1 AND m.account_id = ?2
         ORDER BY bm25(messages_fts), m.date_ts DESC
         LIMIT ?3"
    } else {
        "SELECT
           m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
           m.to_emails, m.date_ts, m.snippet, m.is_read,
           (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS attachment_count
         FROM messages m
         JOIN messages_fts ON messages_fts.rowid = m.rowid
         WHERE messages_fts MATCH ?1
         ORDER BY bm25(messages_fts), m.date_ts DESC
         LIMIT ?2"
    };

    let map_row = |row: &rusqlite::Row<'_>| message_summary_from_row(row);

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = if let Some(account_id) = account_filter {
        stmt.query_map(params![fts_query, account_id, limit], map_row)
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map(params![fts_query, limit], map_row)
            .map_err(|e| e.to_string())?
    };

    collect_message_summaries(conn, rows)
}

fn search_messages_like(
    conn: &rusqlite::Connection,
    query: &str,
    account_filter: Option<&str>,
    limit: i64,
) -> CmdResult<Vec<crate::models::MessageSummary>> {
    use rusqlite::params;

    let pattern = format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );

    let sql = if account_filter.is_some() {
        "SELECT
           m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
           m.to_emails, m.date_ts, m.snippet, m.is_read,
           (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS attachment_count
         FROM messages m
         WHERE m.account_id = ?1
           AND (
             m.subject LIKE ?2 ESCAPE '\\'
             OR m.from_name LIKE ?2 ESCAPE '\\'
             OR m.from_email LIKE ?2 ESCAPE '\\'
             OR m.to_emails LIKE ?2 ESCAPE '\\'
             OR m.snippet LIKE ?2 ESCAPE '\\'
             OR m.body LIKE ?2 ESCAPE '\\'
           )
         ORDER BY m.date_ts DESC
         LIMIT ?3"
    } else {
        "SELECT
           m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
           m.to_emails, m.date_ts, m.snippet, m.is_read,
           (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS attachment_count
         FROM messages m
         WHERE
           m.subject LIKE ?1 ESCAPE '\\'
           OR m.from_name LIKE ?1 ESCAPE '\\'
           OR m.from_email LIKE ?1 ESCAPE '\\'
           OR m.to_emails LIKE ?1 ESCAPE '\\'
           OR m.snippet LIKE ?1 ESCAPE '\\'
           OR m.body LIKE ?1 ESCAPE '\\'
         ORDER BY m.date_ts DESC
         LIMIT ?2"
    };

    let map_row = |row: &rusqlite::Row<'_>| message_summary_from_row(row);

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = if let Some(account_id) = account_filter {
        stmt.query_map(params![account_id, pattern, limit], map_row)
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map(params![pattern, limit], map_row)
            .map_err(|e| e.to_string())?
    };

    collect_message_summaries(conn, rows)
}

fn message_summary_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<crate::models::MessageSummary> {
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
}

fn collect_message_summaries(
    conn: &rusqlite::Connection,
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<crate::models::MessageSummary>,
    >,
) -> CmdResult<Vec<crate::models::MessageSummary>> {
    let mut out = Vec::new();
    for row in rows {
        let mut message = row.map_err(|e| e.to_string())?;
        message.tags = read_message_tags(conn, &message.id)?;
        out.push(message);
    }
    Ok(out)
}

#[tauri::command]
pub fn get_message(
    state: State<'_, crate::state::AppState>,
    message_id: String,
) -> CmdResult<crate::models::MessageDetail> {
    use rusqlite::params;

    let (account_id, folder_path, imap_uid, body_fetched): (String, String, Option<i64>, i64) =
        state.db.with_conn(|conn| {
            conn.query_row(
                "SELECT account_id, folder_path, imap_uid, body_fetched
                 FROM messages
                 WHERE id = ?1",
                params![message_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| e.to_string())
        })?;

    if body_fetched == 0 {
        if let Some(uid) = imap_uid {
            if let Some(settings) = get_account_settings_from_db(&state.db, &account_id)? {
                let auth = get_account_auth(&state.db, &account_id)?;
                let fetched =
                    crate::mail::fetch_imap_message_body(&settings, &auth, &folder_path, uid)?;
                cache_message_body_and_attachment_meta(
                    &state.db,
                    &message_id,
                    &fetched.body,
                    fetched.body_html.as_deref(),
                    &fetched.attachments,
                )?;
            }
        }
    }

    read_message_detail(&state.db, &message_id)
}

fn cache_message_body_and_attachment_meta(
    db: &crate::db::Db,
    message_id: &str,
    body: &str,
    body_html: Option<&str>,
    attachments: &[crate::mail::FetchedAttachmentMeta],
) -> CmdResult<()> {
    use rusqlite::params;
    let now = crate::db::unix_ts_now();
    let snippet = body.lines().next().unwrap_or("").to_string();

    db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        if let Some(html) = body_html {
            tx.execute(
                "UPDATE messages
                 SET body = ?1, body_html = ?2, snippet = ?3, body_fetched = 1
                 WHERE id = ?4",
                params![body, html, snippet, message_id],
            )
            .map_err(|e| e.to_string())?;
        } else {
            tx.execute(
                "UPDATE messages
                 SET body = ?1, snippet = ?2, body_fetched = 1
                 WHERE id = ?3",
                params![body, snippet, message_id],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.execute(
            "DELETE FROM attachments WHERE message_id = ?1 AND content IS NULL",
            params![message_id],
        )
        .map_err(|e| e.to_string())?;

        for attachment in attachments {
            tx.execute(
                "INSERT INTO attachments (
                   id, message_id, filename, mime_type, size_bytes,
                   content, content_id, disposition, attachment_index, created_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9)",
                params![
                    Uuid::new_v4().to_string(),
                    message_id,
                    attachment.filename.as_str(),
                    attachment.mime_type.as_str(),
                    attachment.size_bytes,
                    attachment.content_id.as_deref(),
                    attachment.disposition.as_str(),
                    attachment.attachment_index,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn read_message_detail(
    db: &crate::db::Db,
    message_id: &str,
) -> CmdResult<crate::models::MessageDetail> {
    use rusqlite::params;

    db.with_conn(|conn| {
        let mut message = conn
            .query_row(
                "SELECT id, account_id, folder_path, subject, from_name, from_email, to_emails, date_ts, body, body_html, is_read
                 FROM messages
                 WHERE id = ?1",
                params![message_id],
                |row| {
                    let to_emails_json: String = row.get(6)?;
                    let to_emails: Vec<String> =
                        serde_json::from_str(&to_emails_json).unwrap_or_default();
                    let body_html: Option<String> = row.get(9)?;
                    let is_html = body_html.is_some();
                    let is_read: i64 = row.get(10)?;

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
                        body_html,
                        is_html,
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
    _app: tauri::AppHandle,
    state: State<'_, crate::state::AppState>,
    attachment_id: String,
) -> CmdResult<String> {
    let attachment = read_or_fetch_attachment_content(&state.db, &attachment_id)?;
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("woxmail-data");
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

struct AttachmentRecord {
    id: String,
    filename: String,
    mime_type: String,
    size_bytes: i64,
    content: Option<Vec<u8>>,
    content_id: Option<String>,
    disposition: String,
    attachment_index: Option<i64>,
    account_id: String,
    folder_path: String,
    imap_uid: Option<i64>,
}

fn read_or_fetch_attachment_content(
    db: &crate::db::Db,
    attachment_id: &str,
) -> CmdResult<AttachmentContent> {
    let mut record = read_attachment_record(db, attachment_id)?;
    if record
        .content
        .as_ref()
        .is_some_and(|content| !content.is_empty())
    {
        let content = record.content.take().unwrap_or_default();
        return Ok(AttachmentContent {
            filename: record.filename,
            content,
        });
    }

    let imap_uid = record.imap_uid.ok_or_else(|| {
        "该附件没有本地内容，也缺少远端邮件 UID。请重新同步这封邮件后再试。".to_string()
    })?;
    let settings = get_account_settings_from_db(db, &record.account_id)?
        .ok_or_else(|| "该账户缺少服务器登录设置，无法下载附件。".to_string())?;
    let auth = get_account_auth(db, &record.account_id)?;
    let selector = crate::mail::AttachmentSelector {
        filename: record.filename.clone(),
        mime_type: record.mime_type.clone(),
        size_bytes: record.size_bytes,
        content_id: record.content_id.clone(),
        disposition: record.disposition.clone(),
        attachment_index: record.attachment_index,
    };
    let content = crate::mail::fetch_imap_attachment_content(
        &settings,
        &auth,
        &record.folder_path,
        imap_uid,
        &selector,
    )?;

    if content.is_empty() {
        return Err("附件内容为空，无法保存或打开。".to_string());
    }

    db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE attachments SET content = ?1, size_bytes = ?2 WHERE id = ?3",
            rusqlite::params![content.as_slice(), content.len() as i64, record.id.as_str()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    Ok(AttachmentContent {
        filename: record.filename,
        content,
    })
}

fn read_attachment_record(db: &crate::db::Db, attachment_id: &str) -> CmdResult<AttachmentRecord> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT
               a.id, a.filename, a.mime_type, a.size_bytes, a.content,
               a.content_id, a.disposition, a.attachment_index,
               m.account_id, m.folder_path, m.imap_uid
             FROM attachments a
             JOIN messages m ON m.id = a.message_id
             WHERE a.id = ?1",
            rusqlite::params![attachment_id],
            |row| {
                Ok(AttachmentRecord {
                    id: row.get(0)?,
                    filename: row.get(1)?,
                    mime_type: row.get(2)?,
                    size_bytes: row.get(3)?,
                    content: row.get(4)?,
                    content_id: row.get(5)?,
                    disposition: row.get(6)?,
                    attachment_index: row.get(7)?,
                    account_id: row.get(8)?,
                    folder_path: row.get(9)?,
                    imap_uid: row.get(10)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "附件不存在".to_string(),
            _ => e.to_string(),
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
    let parent = original
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();

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
    let job_id = Uuid::new_v4().to_string();
    let sent_folder_path = input
        .sent_folder_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Sent")
        .to_string();
    let to_emails_json = serde_json::to_string(&input.to_emails).map_err(|e| e.to_string())?;

    state.db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO outbox_jobs (
               id, account_id, to_emails, subject, body, is_html,
               sent_folder_path, status, attempts, next_attempt_at,
               created_at, updated_at
            )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', 0, 0, ?8, ?8)",
            params![
                job_id.as_str(),
                input.account_id.as_str(),
                to_emails_json.as_str(),
                input.subject.as_str(),
                input.body.as_str(),
                if input.is_html.unwrap_or(false) {
                    1i64
                } else {
                    0i64
                },
                sent_folder_path.as_str(),
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    process_outbox_job(&state.db, &job_id)
        .map_err(|error| format!("发送失败，邮件已保存在本地发件箱并会自动重试。{error}"))
}

#[tauri::command]
pub fn process_outbox(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<crate::models::ProcessOutboxResult> {
    let now = crate::db::unix_ts_now();
    let stale_sending_before = now.saturating_sub(5 * 60);
    let job_ids = state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id
                 FROM outbox_jobs
                 WHERE
                   (status IN ('queued', 'failed') AND next_attempt_at <= ?1)
                   OR (status = 'sending' AND updated_at <= ?2)
                 ORDER BY created_at ASC
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![now, stale_sending_before], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })?;

    let mut result = crate::models::ProcessOutboxResult {
        attempted: 0,
        sent: 0,
        failed: 0,
    };

    for job_id in job_ids {
        result.attempted += 1;
        match process_outbox_job(&state.db, &job_id) {
            Ok(_) => result.sent += 1,
            Err(_) => result.failed += 1,
        }
    }

    Ok(result)
}

fn process_outbox_job(db: &crate::db::Db, job_id: &str) -> CmdResult<String> {
    use rusqlite::params;

    let job = read_outbox_job(db, job_id)?;
    let msg_id = Uuid::new_v4().to_string();
    let to_emails_json = serde_json::to_string(&job.to_emails).unwrap_or_else(|_| "[]".to_string());

    mark_outbox_job_sending(db, &job.id)?;
    let send_result = send_outbox_job(db, &job);
    match send_result {
        Ok((from_name, from_email)) => {
            let now = crate::db::unix_ts_now();
            db.with_conn_mut(|conn| {
                let stored_body = if job.is_html {
                    crate::text::html_to_text(&job.body)
                } else {
                    job.body.clone()
                };
                let snippet = stored_body.lines().next().unwrap_or("").to_string();
                conn.execute(
                    "INSERT INTO messages (
                       id, account_id, folder_path, subject, from_name, from_email,
                       to_emails, date_ts, snippet, body, body_fetched, is_read, created_at
                     )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 1, ?11)",
                    params![
                        msg_id.as_str(),
                        job.account_id.as_str(),
                        job.sent_folder_path.as_str(),
                        job.subject.as_str(),
                        from_name.as_str(),
                        from_email.as_str(),
                        to_emails_json,
                        now,
                        snippet,
                        stored_body,
                        now
                    ],
                )
                .map_err(|e| e.to_string())?;
                conn.execute(
                    "DELETE FROM outbox_jobs WHERE id = ?1",
                    params![job.id.as_str()],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })?;
            Ok(msg_id)
        }
        Err(error) => {
            mark_outbox_job_failed(db, &job, &error)?;
            Err(error)
        }
    }
}

fn send_outbox_job(
    db: &crate::db::Db,
    job: &crate::models::OutboxJob,
) -> CmdResult<(String, String)> {
    use rusqlite::params;

    let (from_name, from_email): (String, String) = db.with_conn(|conn| {
        conn.query_row(
            "SELECT name, email FROM accounts WHERE id = ?1",
            params![job.account_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())
    })?;

    let settings = get_account_settings_from_db(db, &job.account_id)?
        .ok_or_else(|| "该账户缺少服务器登录设置，无法发送邮件。".to_string())?;
    let auth = get_account_auth(db, &job.account_id)?;
    let raw_message = crate::mail::send_smtp(
        &settings,
        &auth,
        &from_email,
        &from_name,
        &job.to_emails,
        &job.subject,
        &job.body,
        job.is_html,
    )?;

    let _ = crate::mail::append_imap_message(&settings, &auth, &job.sent_folder_path, &raw_message);

    Ok((from_name, from_email))
}

fn read_outbox_job(db: &crate::db::Db, job_id: &str) -> CmdResult<crate::models::OutboxJob> {
    use rusqlite::params;

    db.with_conn(|conn| {
        conn.query_row(
            "SELECT
               id, account_id, to_emails, subject, body, is_html,
               sent_folder_path, status, attempts, last_error,
               next_attempt_at, created_at, updated_at
             FROM outbox_jobs
             WHERE id = ?1",
            params![job_id],
            |row| {
                let to_emails_json: String = row.get(2)?;
                let to_emails = serde_json::from_str(&to_emails_json).unwrap_or_default();
                let is_html: i64 = row.get(5)?;
                Ok(crate::models::OutboxJob {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    to_emails,
                    subject: row.get(3)?,
                    body: row.get(4)?,
                    is_html: is_html != 0,
                    sent_folder_path: row.get(6)?,
                    status: row.get(7)?,
                    attempts: row.get(8)?,
                    last_error: row.get(9)?,
                    next_attempt_at: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "发件箱任务不存在".to_string(),
            _ => e.to_string(),
        })
    })
}

fn mark_outbox_job_sending(db: &crate::db::Db, job_id: &str) -> CmdResult<()> {
    let now = crate::db::unix_ts_now();
    db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE outbox_jobs
             SET status = 'sending', updated_at = ?1
             WHERE id = ?2",
            rusqlite::params![now, job_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn mark_outbox_job_failed(
    db: &crate::db::Db,
    job: &crate::models::OutboxJob,
    error: &str,
) -> CmdResult<()> {
    let now = crate::db::unix_ts_now();
    let attempts = job.attempts + 1;
    let retry_delay = (30i64 * 2i64.pow((attempts.saturating_sub(1)).min(5) as u32)).min(1800);
    db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE outbox_jobs
             SET status = 'failed',
                 attempts = ?1,
                 last_error = ?2,
                 next_attempt_at = ?3,
                 updated_at = ?4
             WHERE id = ?5",
            rusqlite::params![attempts, error, now + retry_delay, now, job.id.as_str()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn list_outbox_jobs(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<Vec<crate::models::OutboxJob>> {
    use rusqlite::params;

    state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT
                   id, account_id, to_emails, subject, body, is_html,
                   sent_folder_path, status, attempts, last_error,
                   next_attempt_at, created_at, updated_at
                 FROM outbox_jobs
                 ORDER BY
                   CASE status
                     WHEN 'sending' THEN 0
                     WHEN 'pending' THEN 1
                     WHEN 'failed' THEN 2
                   END,
                   created_at DESC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![], |row| {
                let to_emails_json: String = row.get(2)?;
                let to_emails = serde_json::from_str(&to_emails_json).unwrap_or_default();
                let is_html: i64 = row.get(5)?;
                Ok(crate::models::OutboxJob {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    to_emails,
                    subject: row.get(3)?,
                    body: row.get(4)?,
                    is_html: is_html != 0,
                    sent_folder_path: row.get(6)?,
                    status: row.get(7)?,
                    attempts: row.get(8)?,
                    last_error: row.get(9)?,
                    next_attempt_at: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
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
pub fn retry_outbox_job(
    state: State<'_, crate::state::AppState>,
    job_id: String,
) -> CmdResult<()> {
    let now = crate::db::unix_ts_now();
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE outbox_jobs
             SET status = 'pending',
                 next_attempt_at = ?1,
                 updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![now, now, job_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn cancel_outbox_job(
    state: State<'_, crate::state::AppState>,
    job_id: String,
) -> CmdResult<()> {
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "DELETE FROM outbox_jobs WHERE id = ?1",
            rusqlite::params![job_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn get_cache_settings(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<crate::models::CacheSettings> {
    state.db.with_conn(|conn| {
        let row = conn.query_row(
            "SELECT body_retention_days, attachment_max_mb, total_cache_max_mb
             FROM cache_settings WHERE id = 1",
            [],
            |row| {
                Ok(crate::models::CacheSettings {
                    body_retention_days: row.get(0)?,
                    attachment_max_mb: row.get(1)?,
                    total_cache_max_mb: row.get(2)?,
                })
            },
        );
        match row {
            Ok(value) => Ok(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let now = crate::db::unix_ts_now();
                conn.execute(
                    "INSERT INTO cache_settings (id, body_retention_days, attachment_max_mb, total_cache_max_mb, updated_at)
                     VALUES (1, 30, 500, 2000, ?1)",
                    rusqlite::params![now],
                )
                .map_err(|e| e.to_string())?;
                Ok(crate::models::CacheSettings {
                    body_retention_days: 30,
                    attachment_max_mb: 500,
                    total_cache_max_mb: 2000,
                })
            }
            Err(e) => Err(e.to_string()),
        }
    })
}

#[tauri::command]
pub fn set_cache_settings(
    state: State<'_, crate::state::AppState>,
    input: crate::models::CacheSettings,
) -> CmdResult<()> {
    let now = crate::db::unix_ts_now();
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO cache_settings (id, body_retention_days, attachment_max_mb, total_cache_max_mb, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               body_retention_days = excluded.body_retention_days,
               attachment_max_mb = excluded.attachment_max_mb,
               total_cache_max_mb = excluded.total_cache_max_mb,
               updated_at = excluded.updated_at",
            rusqlite::params![
                input.body_retention_days,
                input.attachment_max_mb,
                input.total_cache_max_mb,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn get_cache_stats(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<crate::models::CacheStats> {
    state.db.with_conn(|conn| {
        let message_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap_or(0);
        let attachment_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
            .unwrap_or(0);
        let total_body_bytes: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(body)), 0) FROM messages",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let total_attachment_bytes: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM attachments WHERE content IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let page_count: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap_or(0);
        let page_size: i64 = conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(crate::models::CacheStats {
            message_count,
            attachment_count,
            total_body_bytes,
            total_attachment_bytes,
            db_size_bytes: page_count * page_size,
        })
    })
}

#[tauri::command]
pub fn clear_cache(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<usize> {
    state.db.with_conn_mut(|conn| {
        let deleted = conn
            .execute("DELETE FROM attachments WHERE content IS NOT NULL", [])
            .map_err(|e| e.to_string())?;
        let _ = conn.execute("VACUUM", []).map_err(|e| e.to_string())?;
        Ok(deleted)
    })
}

#[tauri::command]
pub fn purge_old_messages(
    state: State<'_, crate::state::AppState>,
    older_than_days: i64,
) -> CmdResult<usize> {
    let cutoff = crate::db::unix_ts_now() - older_than_days * 86400;
    state.db.with_conn_mut(|conn| {
        let deleted = conn
            .execute(
                "DELETE FROM messages WHERE date_ts < ?1 AND body_fetched = 1",
                rusqlite::params![cutoff],
            )
            .map_err(|e| e.to_string())?;
        let _ = conn.execute("VACUUM", []).map_err(|e| e.to_string())?;
        Ok(deleted)
    })
}

#[tauri::command]
pub fn list_contacts(
    state: State<'_, crate::state::AppState>,
    search: Option<String>,
) -> CmdResult<Vec<crate::models::Contact>> {
    crate::contacts::list_contacts(&state.db, search.as_deref())
}

#[tauri::command]
pub fn create_contact(
    state: State<'_, crate::state::AppState>,
    input: crate::models::CreateContactInput,
) -> CmdResult<crate::models::Contact> {
    crate::contacts::create_contact(&state.db, &input)
}

#[tauri::command]
pub fn update_contact(
    state: State<'_, crate::state::AppState>,
    id: String,
    input: crate::models::UpdateContactInput,
) -> CmdResult<crate::models::Contact> {
    crate::contacts::update_contact(&state.db, &id, &input)
}

#[tauri::command]
pub fn delete_contact(
    state: State<'_, crate::state::AppState>,
    id: String,
) -> CmdResult<()> {
    crate::contacts::delete_contact(&state.db, &id)
}

#[tauri::command]
pub fn import_contacts(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<usize> {
    crate::contacts::import_contacts_from_mail(&state.db)
}

#[tauri::command]
pub fn translate_message(
    state: State<'_, crate::state::AppState>,
    text: String,
    to_lang: Option<String>,
    appid: Option<String>,
    secret: Option<String>,
) -> CmdResult<String> {
    let lang = to_lang.unwrap_or_else(|| "zh".to_string());
    crate::translate::translate_text(&state.db, &text, &lang, appid.as_deref(), secret.as_deref())
}

#[tauri::command]
pub fn import_mbox(
    state: State<'_, crate::state::AppState>,
    account_id: String,
    folder_path: String,
    file_path: String,
) -> CmdResult<usize> {
    crate::imports::import_mbox(&state.db, &account_id, &folder_path, &file_path)
}

#[tauri::command]
pub fn import_eml(
    state: State<'_, crate::state::AppState>,
    account_id: String,
    folder_path: String,
    file_path: String,
) -> CmdResult<usize> {
    crate::imports::import_eml(&state.db, &account_id, &folder_path, &file_path)
}

#[tauri::command]
pub fn list_filter_rules(
    state: State<'_, crate::state::AppState>,
) -> CmdResult<Vec<crate::models::FilterRule>> {
    state.db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, field, operator, value, action_type, action_value, enabled, sort_order, created_at, updated_at
                 FROM filter_rules ORDER BY sort_order",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let enabled: i64 = row.get(7)?;
                Ok(crate::models::FilterRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    field: row.get(2)?,
                    operator: row.get(3)?,
                    value: row.get(4)?,
                    action_type: row.get(5)?,
                    action_value: row.get(6)?,
                    enabled: enabled != 0,
                    sort_order: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows { out.push(row.map_err(|e| e.to_string())?); }
        Ok(out)
    })
}

#[tauri::command]
pub fn create_filter_rule(
    state: State<'_, crate::state::AppState>,
    input: crate::models::CreateFilterRuleInput,
) -> CmdResult<crate::models::FilterRule> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = crate::db::unix_ts_now();
    let rule = crate::models::FilterRule {
        id: id.clone(), name: input.name, field: input.field, operator: input.operator,
        value: input.value, action_type: input.action_type, action_value: input.action_value,
        enabled: true, sort_order: 0, created_at: now, updated_at: now,
    };
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO filter_rules (id, name, field, operator, value, action_type, action_value, enabled, sort_order, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,1,0,?8,?9)",
            rusqlite::params![rule.id, rule.name, rule.field, rule.operator, rule.value, rule.action_type, rule.action_value, now, now],
        ).map_err(|e| e.to_string())?;
        Ok(())
    })?;
    Ok(rule)
}

#[tauri::command]
pub fn delete_filter_rule(
    state: State<'_, crate::state::AppState>,
    rule_id: String,
) -> CmdResult<()> {
    state.db.with_conn_mut(|conn| {
        conn.execute("DELETE FROM filter_rules WHERE id = ?1", rusqlite::params![rule_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
pub fn toggle_filter_rule(
    state: State<'_, crate::state::AppState>,
    rule_id: String,
    enabled: bool,
) -> CmdResult<()> {
    let now = crate::db::unix_ts_now();
    state.db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE filter_rules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![if enabled { 1i64 } else { 0i64 }, now, rule_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[tauri::command]
#[allow(dead_code)]
pub fn get_thread(
    state: State<'_, crate::state::AppState>,
    message_id: String,
    account_id: String,
) -> CmdResult<Vec<crate::models::MessageSummary>> {
    state.db.with_conn(|conn| {
        // Get the subject of the given message (strip Re:/Fwd: prefix)
        let subject: String = conn.query_row(
            "SELECT subject FROM messages WHERE id = ?1",
            rusqlite::params![message_id],
            |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        let base_subject = subject
            .trim_start()
            .trim_start_matches(|c: char| c.is_whitespace())
            .replace("Re:", "").replace("RE:", "").replace("re:", "")
            .replace("Fwd:", "").replace("FWD:", "").replace("fwd:", "")
            .replace("FW:", "").replace("fw:", "")
            .trim().to_string();

        let mut stmt = conn.prepare(
            "SELECT
               m.id, m.account_id, m.folder_path, m.subject, m.from_name, m.from_email,
               m.to_emails, m.date_ts, m.snippet, m.is_read,
               (SELECT COUNT(*) FROM attachments a WHERE a.message_id = m.id) AS attachment_count
             FROM messages m
             WHERE m.account_id = ?1
               AND (
                 REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(m.subject, 'Re:', ''), 'RE:', ''), 're:', ''), 'Fwd:', ''), 'FWD:', ''), 'FW:', '') LIKE ?2
                 OR m.id = ?3
               )
             ORDER BY m.date_ts ASC
             LIMIT 100",
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(
            rusqlite::params![account_id, format!("%{}%", base_subject), message_id],
            |row| message_summary_from_row(row),
        ).map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            let mut msg = row.map_err(|e| e.to_string())?;
            msg.tags = read_message_tags(conn, &msg.id)?;
            out.push(msg);
        }
        Ok(out)
    })
}

#[tauri::command]
pub fn apply_filters(
    state: State<'_, crate::state::AppState>,
    message_id: String,
) -> CmdResult<Vec<String>> {
    use rusqlite::params;
    let db = &state.db;
    let msg = db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, account_id, from_email, from_name, subject, to_emails, folder_path FROM messages WHERE id = ?1",
            params![message_id],
            |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(3)?,
                row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?,
                row.get::<_, String>(2)?,
            )),
        ).map_err(|e| e.to_string())
    })?;
    let (_id, _acct, from_email, from_name, subject, to_json, folder): (String,String,String,String,String,String,String) = msg;

    let rules = db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, field, operator, value, action_type, action_value FROM filter_rules WHERE enabled = 1 ORDER BY sort_order"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
        ))).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows { out.push(row.map_err(|e| e.to_string())?); }
        Ok::<_, String>(out)
    })?;

    let mut applied = Vec::new();
    for (_rid, field, op, val, action, action_val) in rules {
        let test = match field.as_str() {
            "from" | "from_email" => &from_email,
            "from_name" => &from_name,
            "subject" => &subject,
            "to" => &to_json,
            _ => &subject,
        };
        let matches = match op.as_str() {
            "contains" => test.to_lowercase().contains(&val.to_lowercase()),
            "equals" => test.eq_ignore_ascii_case(&val),
            "starts_with" => test.to_lowercase().starts_with(&val.to_lowercase()),
            _ => false,
        };
        if matches {
            match action.as_str() {
                "tag" => {
                    let _ = add_single_message_tag(db, &message_id, &action_val);
                    applied.push(format!("标签: {}", action_val));
                }
                "move" => {
                    let target = if action_val.is_empty() { folder.clone() } else { action_val.clone() };
                    let _ = move_single_message(db, &message_id, &target);
                    applied.push(format!("移动到: {}", target));
                }
                _ => {}
            }
        }
    }
    Ok(applied)
}

fn add_single_message_tag(db: &crate::db::Db, msg_id: &str, tag: &str) -> Result<(), String> {
    let now = crate::db::unix_ts_now();
    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT OR IGNORE INTO message_tags (message_id, tag, created_at) VALUES (?1,?2,?3)",
            rusqlite::params![msg_id, tag, now],
        ).map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn move_single_message(db: &crate::db::Db, msg_id: &str, target: &str) -> Result<(), String> {
    db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE messages SET folder_path = ?1 WHERE id = ?2",
            rusqlite::params![target, msg_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn apply_filters_to_new_messages(db: &crate::db::Db, account_id: &str) -> Result<usize, String> {
    let rules: Vec<(String, String, String, String, String)> = db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT field, operator, value, action_type, action_value FROM filter_rules WHERE enabled = 1 ORDER BY sort_order"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            row.get::<_, String>(3)?, row.get::<_, String>(4)?,
        ))).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows { out.push(row.map_err(|e| e.to_string())?); }
        Ok::<_, String>(out)
    })?;

    if rules.is_empty() { return Ok(0); }

    // Get recent messages for this account (last 50)
    let msgs: Vec<(String, String, String, String)> = db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, from_email, from_name, subject FROM messages WHERE account_id = ?1 ORDER BY date_ts DESC LIMIT 50"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![account_id], |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?,
        ))).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows { out.push(row.map_err(|e| e.to_string())?); }
        Ok::<_, String>(out)
    })?;

    let mut count = 0usize;
    for (msg_id, from_email, from_name, subject) in msgs {
        for (field, op, val, action, action_val) in &rules {
            let test = match field.as_str() {
                "from" | "from_email" => &from_email,
                "from_name" => &from_name,
                "subject" => &subject,
                _ => &subject,
            };
            let matches = match op.as_str() {
                "contains" => test.to_lowercase().contains(&val.to_lowercase()),
                "equals" => test.eq_ignore_ascii_case(val),
                "starts_with" => test.to_lowercase().starts_with(&val.to_lowercase()),
                _ => false,
            };
            if matches {
                match action.as_str() {
                    "tag" => {
                        let _ = add_single_message_tag(db, &msg_id, action_val);
                        count += 1;
                    }
                    "move" => {
                        let _ = move_single_message(db, &msg_id, action_val);
                        count += 1;
                    }
                    _ => {}
                }
                break;
            }
        }
    }
    Ok(count)
}
