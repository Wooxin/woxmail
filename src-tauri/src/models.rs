use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountInput {
    pub provider: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailFolder {
    pub id: String,
    pub account_id: String,
    pub path: String,
    pub name: String,
    pub delimiter: Option<String>,
    pub selectable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnreadCount {
    pub account_id: String,
    pub folder_path: String,
    pub unread_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: String,
    pub account_id: String,
    pub folder_path: String,
    pub subject: String,
    pub from_name: String,
    pub from_email: String,
    pub to_emails: Vec<String>,
    pub date_ts: i64,
    pub snippet: String,
    pub is_read: bool,
    pub attachment_count: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDetail {
    pub id: String,
    pub account_id: String,
    pub folder_path: String,
    pub subject: String,
    pub from_name: String,
    pub from_email: String,
    pub to_emails: Vec<String>,
    pub date_ts: i64,
    pub body: String,
    #[serde(default)]
    pub body_html: Option<String>,
    #[serde(default)]
    pub is_html: bool,
    pub is_read: bool,
    pub attachments: Vec<MessageAttachment>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttachment {
    pub id: String,
    pub message_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub content_id: Option<String>,
    pub disposition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageInput {
    pub account_id: String,
    pub to_emails: Vec<String>,
    pub subject: String,
    pub body: String,
    pub sent_folder_path: Option<String>,
    pub is_html: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    pub id: String,
    pub account_id: String,
    pub to_emails: Vec<String>,
    pub subject: String,
    pub body: String,
    pub is_html: bool,
    pub sent_folder_path: String,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub next_attempt_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOutboxResult {
    pub attempted: i64,
    pub sent: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeDraft {
    pub scope: String,
    pub account_id: String,
    pub to_emails: Vec<String>,
    pub subject: String,
    pub body: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveComposeDraftInput {
    pub scope: String,
    pub account_id: String,
    pub to_emails: Vec<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSettings {
    pub account_id: String,
    pub provider: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_tls: bool,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_tls: bool,
    pub smtp_username: String,
    pub has_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetAccountSettingsInput {
    pub account_id: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_tls: bool,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_tls: bool,
    pub smtp_username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailOAuthLoginInput {
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlookOAuthLoginInput {
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettings {
    pub body_retention_days: i64,
    pub attachment_max_mb: i64,
    pub total_cache_max_mb: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub message_count: i64,
    pub attachment_count: i64,
    pub total_body_bytes: i64,
    pub total_attachment_bytes: i64,
    pub db_size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContactInput {
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateContactInput {
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    pub id: String,
    pub name: String,
    pub field: String,
    pub operator: String,
    pub value: String,
    pub action_type: String,
    pub action_value: String,
    pub enabled: bool,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFilterRuleInput {
    pub name: String,
    pub field: String,
    pub operator: String,
    pub value: String,
    pub action_type: String,
    pub action_value: String,
}
