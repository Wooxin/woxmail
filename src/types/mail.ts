export type MailProvider =
	| "gmail"
	| "outlook"
	| "qq"
	| "proton"
	| "icloud"
	| "netease"
	| "imap"

export type MailFolder = string

export interface MailFolderInfo {
	id: string
	account_id: string
	path: string
	name: string
	delimiter: string | null
	selectable: boolean
}

export interface MailAccount {
	id: string

	provider: MailProvider

	name: string

	email: string
}

export interface UnreadCount {
	account_id: string
	folder_path: string
	unread_count: number
}

export interface MessageSummary {
	id: string
	account_id: string
	folder_path: string
	subject: string
	from_name: string
	from_email: string
	to_emails: string[]
	date_ts: number
	snippet: string
	is_read: boolean
	attachment_count: number
	tags: string[]
}

export interface MessageDetail {
	id: string
	account_id: string
	folder_path: string
	subject: string
	from_name: string
	from_email: string
	to_emails: string[]
	date_ts: number
	body: string
	body_html?: string | null
	is_html: boolean
	is_read: boolean
	attachments: MessageAttachment[]
	tags: string[]
}

export interface MessageAttachment {
	id: string
	message_id: string
	filename: string
	mime_type: string
	size_bytes: number
	content_id: string | null
	disposition: string
}

export interface SendMessageInput {
	account_id: string
	to_emails: string[]
	subject: string
	body: string
	sent_folder_path?: string
	is_html?: boolean
}

export interface ComposeDraft {
	scope: string
	account_id: string
	to_emails: string[]
	subject: string
	body: string
	updated_at: number
}

export interface AccountSettings {
	account_id: string
	provider: string
	imap_host: string
	imap_port: number
	imap_tls: boolean
	imap_username: string
	smtp_host: string
	smtp_port: number
	smtp_tls: boolean
	smtp_username: string
	has_password: boolean
}

export interface OutboxJob {
	id: string
	account_id: string
	to_emails: string[]
	subject: string
	body: string
	is_html: boolean
	sent_folder_path: string
	status: string
	attempts: number
	last_error: string | null
	next_attempt_at: number
	created_at: number
	updated_at: number
}

export interface CacheSettings {
	body_retention_days: number
	attachment_max_mb: number
	total_cache_max_mb: number
}

export interface CacheStats {
	message_count: number
	attachment_count: number
	total_body_bytes: number
	total_attachment_bytes: number
	db_size_bytes: number
}

export interface Contact {
	id: string
	name: string
	email: string
	phone?: string | null
	notes?: string | null
	avatar_url?: string | null
	created_at: number
	updated_at: number
}

export interface CreateContactInput {
	name: string
	email: string
	phone?: string | null
	notes?: string | null
}
