import type {
  MailAccount,
  MailFolderInfo,
  AccountSettings,
  MessageDetail,
  MessageSummary,
  SendMessageInput,
  UnreadCount,
} from "../types/mail"
import { tauriInvoke } from "./tauri"

export async function listAccounts(): Promise<MailAccount[]> {
  return tauriInvoke<MailAccount[]>(
    "list_accounts",
  )
}

export async function createAccount(input: {
  provider: string
  name: string
  email: string
}): Promise<MailAccount> {
  return tauriInvoke<MailAccount>(
    "create_account",
    { input },
  )
}

export async function gmailOAuthLogin(): Promise<MailAccount> {
  return tauriInvoke<MailAccount>(
    "gmail_oauth_login",
    {
      input: {},
    },
  )
}

export async function outlookOAuthLogin(): Promise<MailAccount> {
  return tauriInvoke<MailAccount>(
    "outlook_oauth_login",
    {
      input: {},
    },
  )
}

export async function deleteAccount(accountId: string): Promise<void> {
  await tauriInvoke<void>(
    "delete_account",
    { accountId },
  )
}

export async function syncFolder(accountId: string, folderPath: string, notify = true): Promise<number> {
  return tauriInvoke<number>(
    "sync_folder",
    { accountId, folderPath, notify },
  )
}

export async function listFolders(accountId: string): Promise<MailFolderInfo[]> {
  return tauriInvoke<MailFolderInfo[]>(
    "list_folders",
    { accountId },
  )
}

export async function listMessages(input: {
  accountId: string
  folderPath: string
  limit?: number
  offset?: number
}): Promise<MessageSummary[]> {
  return tauriInvoke<MessageSummary[]>(
    "list_messages",
    {
      accountId: input.accountId,
      folderPath: input.folderPath,
      limit: input.limit,
      offset: input.offset,
    },
  )
}

export async function listUnreadCounts(): Promise<UnreadCount[]> {
  return tauriInvoke<UnreadCount[]>(
    "list_unread_counts",
  )
}

export async function getMessage(messageId: string): Promise<MessageDetail> {
  return tauriInvoke<MessageDetail>(
    "get_message",
    { messageId },
  )
}

export async function markMessageRead(messageId: string): Promise<void> {
  await tauriInvoke<void>(
    "mark_message_read",
    { messageId },
  )
}

export async function addMessageTag(messageIds: string[], tag: string): Promise<void> {
  await tauriInvoke<void>(
    "add_message_tag",
    { messageIds, tag },
  )
}

export async function clearMessageTags(messageIds: string[]): Promise<void> {
  await tauriInvoke<void>(
    "clear_message_tags",
    { messageIds },
  )
}

export async function moveMessagesToFolder(messageIds: string[], folderPath: string): Promise<void> {
  await tauriInvoke<void>(
    "move_messages_to_folder",
    { messageIds, folderPath },
  )
}

export async function sendMessage(input: SendMessageInput): Promise<string> {
  return tauriInvoke<string>(
    "send_message",
    { input },
  )
}

export async function getAccountSettings(accountId: string): Promise<AccountSettings | null> {
  return tauriInvoke<AccountSettings | null>(
    "get_account_settings",
    { accountId },
  )
}

export async function setAccountSettings(input: {
  accountId: string
  imapHost: string
  imapPort: number
  imapTls: boolean
  imapUsername: string
  smtpHost: string
  smtpPort: number
  smtpTls: boolean
  smtpUsername: string
  password: string
}): Promise<void> {
  await tauriInvoke<void>(
    "set_account_settings",
    {
      input: {
        account_id: input.accountId,
        imap_host: input.imapHost,
        imap_port: input.imapPort,
        imap_tls: input.imapTls,
        imap_username: input.imapUsername,
        smtp_host: input.smtpHost,
        smtp_port: input.smtpPort,
        smtp_tls: input.smtpTls,
        smtp_username: input.smtpUsername,
        password: input.password,
      },
    },
  )
}
