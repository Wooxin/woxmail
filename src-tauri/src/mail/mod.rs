use crate::models::AccountSettings;
use base64::Engine;
use lettre::{
    message::{header::ContentType, Mailbox, Message},
    transport::smtp::authentication::{Credentials, Mechanism},
    SmtpTransport, Transport,
};
use mailparse::{DispositionType, MailHeaderMap, ParsedMail};
use native_tls::TlsConnector;
use rusqlite::{params, params_from_iter};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use uuid::Uuid;

struct AttachmentMeta {
    filename: String,
    mime_type: String,
    size_bytes: i64,
    content_id: Option<String>,
    disposition: String,
}

pub struct RemoteFolder {
    pub path: String,
    pub name: String,
    pub delimiter: Option<String>,
    pub selectable: bool,
}

pub enum MailAuth {
    Password(String),
    OAuth2(String),
}

impl MailAuth {
    fn secret(&self) -> &str {
        match self {
            MailAuth::Password(value) | MailAuth::OAuth2(value) => value,
        }
    }

    fn is_oauth2(&self) -> bool {
        matches!(self, MailAuth::OAuth2(_))
    }
}

struct OAuth2ImapAuth {
    user: String,
    access_token: String,
}

impl imap::Authenticator for OAuth2ImapAuth {
    type Response = String;

    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

fn header_value(parsed: &ParsedMail, name: &str) -> Option<String> {
    parsed
        .headers
        .get_first_value(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_from(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let (Some(l), Some(r)) = (raw.rfind('<'), raw.rfind('>')) {
        if l < r {
            let email = raw[l + 1..r].trim().to_string();
            let name = raw[..l].trim().trim_matches('"').to_string();
            return (if name.is_empty() { email.clone() } else { name }, email);
        }
    }
    (raw.to_string(), raw.to_string())
}

fn map_imap_login_error(error: impl ToString) -> String {
    let message = error.to_string();
    let lower = message.to_lowercase();

    if lower.contains("application-specific password required")
        || lower.contains("app password")
        || lower.contains("support.google.com/accounts/answer/185833")
    {
        return "Gmail 要求使用应用专用密码，不能直接填写 Google 账号密码。请先在 Google 账号中开启两步验证，然后生成 App Password，并把生成的 16 位应用专用密码填到这里。帮助：https://support.google.com/accounts/answer/185833".to_string();
    }

    if lower.contains("authenticate failed") || lower.contains("authenticationfailed") {
        return "邮箱服务器拒绝 IMAP 登录。请确认邮箱已开启 IMAP/SMTP，并使用该服务商要求的授权码或应用专用密码。Outlook/Microsoft 账号建议使用“快捷登录（OAuth 登录）”。".to_string();
    }

    message
}

fn decode_imap_modified_utf7(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;

    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after_amp = &rest[start + 1..];

        if after_amp.starts_with('-') {
            out.push('&');
            rest = &after_amp[1..];
            continue;
        }

        let Some(end) = after_amp.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };

        let encoded = &after_amp[..end];
        let mut standard = encoded.replace(',', "/");
        while standard.len() % 4 != 0 {
            standard.push('=');
        }

        match base64::engine::general_purpose::STANDARD.decode(standard) {
            Ok(bytes) => {
                let units = bytes
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                    .collect::<Vec<_>>();
                match String::from_utf16(&units) {
                    Ok(decoded) => out.push_str(&decoded),
                    Err(_) => out.push_str(&rest[start..start + end + 2]),
                }
            }
            Err(_) => out.push_str(&rest[start..start + end + 2]),
        }

        rest = &after_amp[end + 1..];
    }

    out.push_str(rest);
    out
}

fn extract_text_body(parsed: &ParsedMail) -> String {
    if parsed.subparts.is_empty() {
        return parsed.get_body().unwrap_or_default();
    }
    for p in &parsed.subparts {
        if p.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
            return p.get_body().unwrap_or_default();
        }
    }
    for p in &parsed.subparts {
        if p.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            return p.get_body().unwrap_or_default();
        }
    }
    parsed
        .subparts
        .first()
        .map(extract_text_body)
        .unwrap_or_default()
}

fn extract_attachments(parsed: &ParsedMail) -> Vec<AttachmentMeta> {
    let mut attachments = Vec::new();
    collect_attachments(parsed, &mut attachments);
    attachments
}

fn collect_attachments(parsed: &ParsedMail, attachments: &mut Vec<AttachmentMeta>) {
    for part in &parsed.subparts {
        collect_attachments(part, attachments);
    }

    if !parsed.subparts.is_empty() {
        return;
    }

    let disposition = parsed.get_content_disposition();
    let filename = disposition
        .params
        .get("filename")
        .or_else(|| parsed.ctype.params.get("name"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let is_attachment = matches!(disposition.disposition, DispositionType::Attachment);
    if !is_attachment && filename.is_none() {
        return;
    }

    let size_bytes = parsed
        .get_body_raw()
        .map(|body| body.len() as i64)
        .unwrap_or_else(|_| parsed.raw_bytes.len() as i64);
    let disposition_name = match disposition.disposition {
        DispositionType::Inline => "inline".to_string(),
        DispositionType::Attachment => "attachment".to_string(),
        DispositionType::FormData => "form-data".to_string(),
        DispositionType::Extension(value) => value,
    };

    attachments.push(AttachmentMeta {
        filename: filename.unwrap_or_else(|| "attachment".to_string()),
        mime_type: parsed.ctype.mimetype.clone(),
        size_bytes,
        content_id: header_value(parsed, "Content-ID").map(|v| {
            v.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        }),
        disposition: disposition_name,
    });
}

pub fn sync_imap_folder(
    db: &crate::db::Db,
    account_id: &str,
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    limit: usize,
) -> Result<usize, String> {
    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let session = authenticate_imap_client(client, settings, auth)?;
        return sync_imap_session(db, account_id, session, folder_path, limit);
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let session = authenticate_imap_client(client, settings, auth)?;

    sync_imap_session(db, account_id, session, folder_path, limit)
}

pub fn list_imap_folders(
    settings: &AccountSettings,
    auth: &MailAuth,
) -> Result<Vec<RemoteFolder>, String> {
    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        let folders = list_session_folders(&mut session)?;
        let _ = session.logout();
        return Ok(folders);
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    let folders = list_session_folders(&mut session)?;
    let _ = session.logout();
    Ok(folders)
}

fn authenticate_imap_client<T: Read + Write>(
    client: imap::Client<T>,
    settings: &AccountSettings,
    auth: &MailAuth,
) -> Result<imap::Session<T>, String> {
    match auth {
        MailAuth::Password(password) => client
            .login(settings.imap_username.as_str(), password)
            .map_err(|(e, _)| map_imap_login_error(e)),
        MailAuth::OAuth2(access_token) => {
            let oauth = OAuth2ImapAuth {
                user: settings.imap_username.clone(),
                access_token: access_token.clone(),
            };
            client
                .authenticate("XOAUTH2", &oauth)
                .map_err(|(e, _)| map_imap_login_error(e))
        }
    }
}

fn list_session_folders<T: Read + Write>(
    session: &mut imap::Session<T>,
) -> Result<Vec<RemoteFolder>, String> {
    let names = session.list(None, Some("*")).map_err(|e| e.to_string())?;
    let mut out = Vec::new();

    for item in names.iter() {
        let path = item.name().to_string();
        let delimiter = item.delimiter().map(|v| v.to_string());
        let raw_name = delimiter
            .as_deref()
            .and_then(|d| path.rsplit(d).next())
            .filter(|v| !v.is_empty())
            .unwrap_or(path.as_str())
            .to_string();
        let name = decode_imap_modified_utf7(&raw_name);
        let selectable = !item
            .attributes()
            .iter()
            .any(|attr| matches!(attr, imap::types::NameAttribute::NoSelect));

        out.push(RemoteFolder {
            path,
            name,
            delimiter,
            selectable,
        });
    }

    out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(out)
}

fn sync_imap_session<T: Read + Write>(
    db: &crate::db::Db,
    account_id: &str,
    mut session: imap::Session<T>,
    folder_path: &str,
    limit: usize,
) -> Result<usize, String> {
    session.select(folder_path).map_err(|e| e.to_string())?;

    let last_uid = db.with_conn(|conn| {
        conn.query_row(
            "SELECT last_uid FROM folder_sync_state WHERE account_id = ?1 AND folder_path = ?2",
            params![account_id, folder_path],
            |row| row.get::<_, i64>(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(0),
            _ => Err(e),
        })
        .map_err(|e| e.to_string())
    })?;

    let query = if last_uid > 0 {
        format!("UID {}:*", last_uid + 1)
    } else {
        "ALL".to_string()
    };
    let uids = session.uid_search(query).map_err(|e| e.to_string())?;
    let mut uid_vec: Vec<u32> = uids.into_iter().collect();
    uid_vec.sort();
    let start = if last_uid > 0 {
        0
    } else {
        uid_vec.len().saturating_sub(limit)
    };
    let mut targets: Vec<u32> = uid_vec[start..].to_vec();

    if targets.is_empty() {
        let _ = session.logout();
        return Ok(0);
    }

    let existing_uids = existing_imap_uids(db, account_id, folder_path, &targets)?;
    targets.retain(|uid| !existing_uids.contains(&(*uid as i64)));

    if targets.is_empty() {
        if let Some(max_uid) = uid_vec.iter().max() {
            update_folder_sync_state(db, account_id, folder_path, (*max_uid) as i64, last_uid)?;
        }
        let _ = session.logout();
        return Ok(0);
    }

    let mut inserted = 0usize;
    let mut max_seen_uid = last_uid;

    for chunk in targets.chunks(25).rev() {
        let fetch_set = chunk
            .iter()
            .rev()
            .map(|uid| uid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetches = session
            .uid_fetch(fetch_set, "(RFC822 FLAGS UID)")
            .map_err(|e| e.to_string())?;

        for f in fetches.iter() {
            let imap_uid = f.uid.unwrap_or(0);
            if imap_uid == 0 {
                continue;
            }

            let bytes = match f.body() {
                Some(v) => v,
                None => continue,
            };

            let parsed = mailparse::parse_mail(bytes).map_err(|e| e.to_string())?;
            let subject =
                header_value(&parsed, "Subject").unwrap_or_else(|| "(no subject)".to_string());
            let from_raw = header_value(&parsed, "From").unwrap_or_else(|| "unknown".to_string());
            let (from_name, from_email) = parse_from(&from_raw);
            let date_ts = header_value(&parsed, "Date")
                .and_then(|v| mailparse::dateparse(&v).ok())
                .map(|t| t as i64)
                .unwrap_or_else(crate::db::unix_ts_now);
            let body = extract_text_body(&parsed);
            let attachments = extract_attachments(&parsed);
            let snippet = body.lines().next().unwrap_or("").to_string();
            let is_read = f
                .flags()
                .iter()
                .any(|v| format!("{v}").eq_ignore_ascii_case("\\Seen"));
            let to_emails = header_value(&parsed, "To")
                .map(|v| serde_json::to_string(&vec![v]).unwrap_or_else(|_| "[]".to_string()))
                .unwrap_or_else(|| "[]".to_string());
            let source_id = header_value(&parsed, "Message-Id")
                .or_else(|| header_value(&parsed, "Message-ID"))
                .map(|v| format!("imap:msgid:{v}"))
                .unwrap_or_else(|| format!("imap:uid:{imap_uid}"));

            let now = crate::db::unix_ts_now();
            let msg_id = Uuid::new_v4().to_string();

            let did_insert = db.with_conn_mut(|conn| {
                let tx = conn.transaction().map_err(|e| e.to_string())?;
                let changed = tx
                    .execute(
                        "INSERT INTO messages (id, account_id, folder_path, subject, from_name, from_email, to_emails, date_ts, snippet, body, is_read, created_at, source_id, imap_uid)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(account_id, folder_path, imap_uid) DO NOTHING",
                        params![
                            msg_id,
                            account_id,
                            folder_path,
                            subject,
                            from_name,
                            from_email,
                            to_emails,
                            date_ts,
                            snippet,
                            body,
                            if is_read { 1i64 } else { 0i64 },
                            now,
                            source_id,
                            imap_uid as i64
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                if changed > 0 {
                    for attachment in &attachments {
                        tx.execute(
                            "INSERT INTO attachments (id, message_id, filename, mime_type, size_bytes, content_id, disposition, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                            params![
                                Uuid::new_v4().to_string(),
                                msg_id,
                                attachment.filename,
                                attachment.mime_type,
                                attachment.size_bytes,
                                attachment.content_id,
                                attachment.disposition,
                                now
                            ],
                        )
                        .map_err(|e| e.to_string())?;
                    }
                }

                tx.commit().map_err(|e| e.to_string())?;
                Ok(changed > 0)
            })?;

            if did_insert {
                inserted += 1;
            }
            max_seen_uid = max_seen_uid.max(imap_uid as i64);
        }
    }

    update_folder_sync_state(db, account_id, folder_path, max_seen_uid, last_uid)?;

    let _ = session.logout();
    Ok(inserted)
}

fn existing_imap_uids(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    uids: &[u32],
) -> Result<HashSet<i64>, String> {
    if uids.is_empty() {
        return Ok(HashSet::new());
    }

    db.with_conn(|conn| {
        let placeholders = std::iter::repeat("?")
            .take(uids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT imap_uid FROM messages WHERE account_id = ? AND folder_path = ? AND imap_uid IN ({placeholders})"
        );
        let params = std::iter::once(account_id.to_string())
            .chain(std::iter::once(folder_path.to_string()))
            .chain(uids.iter().map(|uid| (*uid as i64).to_string()));
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        let mut out = HashSet::new();
        for row in rows {
            out.insert(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

fn update_folder_sync_state(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    max_seen_uid: i64,
    last_uid: i64,
) -> Result<(), String> {
    if max_seen_uid <= last_uid {
        return Ok(());
    }

    let now = crate::db::unix_ts_now();
    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO folder_sync_state (account_id, folder_path, last_uid, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, folder_path) DO UPDATE SET
               last_uid = MAX(folder_sync_state.last_uid, excluded.last_uid),
               updated_at = excluded.updated_at",
            params![account_id, folder_path, max_seen_uid, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn mark_imap_message_seen(
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    imap_uid: i64,
) -> Result<(), String> {
    if imap_uid <= 0 {
        return Ok(());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        mark_session_message_seen(&mut session, folder_path, imap_uid)?;
        let _ = session.logout();
        return Ok(());
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    mark_session_message_seen(&mut session, folder_path, imap_uid)?;
    let _ = session.logout();
    Ok(())
}

pub fn move_imap_messages(
    settings: &AccountSettings,
    auth: &MailAuth,
    source_folder: &str,
    target_folder: &str,
    imap_uids: &[i64],
) -> Result<(), String> {
    let uid_set = imap_uid_set(imap_uids);
    if uid_set.is_empty() {
        return Ok(());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        move_session_messages(&mut session, source_folder, target_folder, &uid_set)?;
        let _ = session.logout();
        return Ok(());
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    move_session_messages(&mut session, source_folder, target_folder, &uid_set)?;
    let _ = session.logout();
    Ok(())
}

fn mark_session_message_seen<T: Read + Write>(
    session: &mut imap::Session<T>,
    folder_path: &str,
    imap_uid: i64,
) -> Result<(), String> {
    session.select(folder_path).map_err(|e| e.to_string())?;
    session
        .uid_store(imap_uid.to_string(), "+FLAGS.SILENT (\\Seen)")
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn move_session_messages<T: Read + Write>(
    session: &mut imap::Session<T>,
    source_folder: &str,
    target_folder: &str,
    uid_set: &str,
) -> Result<(), String> {
    session.select(source_folder).map_err(|e| e.to_string())?;

    match session.uid_mv(uid_set, target_folder) {
        Ok(_) => Ok(()),
        Err(move_error) => {
            session
                .uid_copy(uid_set, target_folder)
                .map_err(|copy_error| {
                    format!(
                        "服务器移动邮件失败：{move_error}；复制到目标文件夹也失败：{copy_error}"
                    )
                })?;
            session
                .uid_store(uid_set, "+FLAGS.SILENT (\\Deleted)")
                .map_err(|e| e.to_string())?;
            session.uid_expunge(uid_set).map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

fn imap_uid_set(imap_uids: &[i64]) -> String {
    let mut uids = imap_uids
        .iter()
        .copied()
        .filter(|uid| *uid > 0)
        .collect::<Vec<_>>();
    uids.sort_unstable();
    uids.dedup();
    uids.iter()
        .map(|uid| uid.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn send_smtp(
    settings: &AccountSettings,
    auth: &MailAuth,
    from_email: &str,
    from_name: &str,
    to_emails: &[String],
    subject: &str,
    body: &str,
) -> Result<(), String> {
    let from_mb: Mailbox = format!("{from_name} <{from_email}>")
        .parse::<Mailbox>()
        .map_err(|e| e.to_string())?;

    let mut builder = Message::builder().from(from_mb).subject(subject);
    for to in to_emails {
        let mb: Mailbox = to.parse::<Mailbox>().map_err(|e| e.to_string())?;
        builder = builder.to(mb);
    }

    let email = builder
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| e.to_string())?;

    let creds = Credentials::new(settings.smtp_username.clone(), auth.secret().to_string());

    let mut transport_builder = if settings.smtp_port == 465 {
        SmtpTransport::relay(settings.smtp_host.as_str())
            .map_err(|e| e.to_string())?
            .tls(lettre::transport::smtp::client::Tls::Wrapper(
                lettre::transport::smtp::client::TlsParameters::new(settings.smtp_host.clone())
                    .map_err(|e| e.to_string())?,
            ))
    } else if settings.smtp_tls {
        SmtpTransport::starttls_relay(settings.smtp_host.as_str()).map_err(|e| e.to_string())?
    } else {
        SmtpTransport::builder_dangerous(settings.smtp_host.as_str())
    };

    transport_builder = transport_builder
        .port(settings.smtp_port as u16)
        .credentials(creds);
    if auth.is_oauth2() {
        transport_builder = transport_builder.authentication(vec![Mechanism::Xoauth2]);
    }
    let transport = transport_builder.build();
    transport.send(&email).map_err(|e| e.to_string())?;
    Ok(())
}
