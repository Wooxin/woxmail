import { useEffect, useMemo, useState, useTransition } from "react"

import type {
  MailAccount,
  MailFolder,
  MailFolderInfo,
  MailProvider,
  UnreadCount,
} from "./types/mail"

import {
  batchInit,
  createAccount,
  deleteAccount,
  gmailOAuthLogin,
  getAccountSettings,
  listFolders,
  listUnreadCounts,
  outlookOAuthLogin,
  processOutbox,
  setAccountSettings,
  syncFolder,
  syncInboxes,
} from "./api/mail"
import { isTauriRuntime } from "./api/tauri"

import Sidebar from "./components/Sidebar"
import TitleBar from "./components/TitleBar"
import AddAccountModal from "./components/AddAccountModal"
import MailScreen from "./components/MailScreen"
import OutboxScreen from "./components/OutboxScreen"
import ContactScreen from "./components/ContactScreen"
import AddImapAccountModal from "./components/AddImapAccountModal"
import SettingsModal from "./components/SettingsModal"
import type { ImapAccountInput } from "./components/AddImapAccountModal"
import type { AddAccountLoginMode } from "./components/AddAccountModal"
import { useKeyboardShortcuts, type ShortcutAction } from "./hooks/useKeyboardShortcuts"
import { isSelectableFolder } from "./utils/folders"

type AccountFilterId = "all" | string
type SidebarAccountMode = "list" | "dropdown"
const inboxBackgroundSyncMs = 5 * 60_000
const outboxBackgroundRetryMs = 60_000

const providerPresets: Record<
  MailProvider,
  {
    name: string
    values: Partial<ImapAccountInput>
    loginHint: string
    passwordLabel: string
  }
> = {
  gmail: {
    name: "Gmail",
    loginHint: "自定登录已填好 Gmail IMAP/SMTP。Gmail 不能填写 Google 账号密码；请开启两步验证后使用应用专用密码。",
    passwordLabel: "Google 16 位应用专用密码（备用）",
    values: {
      imapHost: "imap.gmail.com",
      imapPort: 993,
      imapTls: true,
      smtpHost: "smtp.gmail.com",
      smtpPort: 465,
      smtpTls: true,
    },
  },
  outlook: {
    name: "Outlook",
    loginHint: "Outlook/Microsoft 推荐使用快捷登录。账号密码登录仅适用于明确允许 IMAP 密码登录的企业邮箱。",
    passwordLabel: "Outlook 密码 / 应用密码（可能不可用）",
    values: {
      imapHost: "outlook.office365.com",
      imapPort: 993,
      imapTls: true,
      smtpHost: "smtp.office365.com",
      smtpPort: 587,
      smtpTls: true,
    },
  },
  qq: {
    name: "QQ Mail",
    loginHint: "自定登录已填好 QQ 邮箱服务器。请在 QQ 邮箱设置中开启 IMAP/SMTP，并使用授权码登录。",
    passwordLabel: "QQ 邮箱授权码",
    values: {
      imapHost: "imap.qq.com",
      imapPort: 993,
      imapTls: true,
      smtpHost: "smtp.qq.com",
      smtpPort: 465,
      smtpTls: true,
    },
  },
  netease: {
    name: "163 / 126",
    loginHint: "自定登录已填好网易邮箱服务器。请开启 IMAP/SMTP 服务，并使用客户端授权码登录。",
    passwordLabel: "网易邮箱授权码",
    values: {
      imapHost: "imap.163.com",
      imapPort: 993,
      imapTls: true,
      smtpHost: "smtp.163.com",
      smtpPort: 465,
      smtpTls: true,
    },
  },
  icloud: {
    name: "iCloud",
    loginHint: "自定登录已填好 iCloud 邮箱服务器。请使用 Apple 账户的 App 专用密码。",
    passwordLabel: "Apple App 专用密码",
    values: {
      imapHost: "imap.mail.me.com",
      imapPort: 993,
      imapTls: true,
      smtpHost: "smtp.mail.me.com",
      smtpPort: 587,
      smtpTls: true,
    },
  },
  proton: {
    name: "Proton Bridge",
    loginHint: "自定登录使用 Proton Mail Bridge 本地端口。请先启动 Bridge，并使用 Bridge 提供的本地密码。",
    passwordLabel: "Bridge 密码",
    values: {
      imapHost: "127.0.0.1",
      imapPort: 1143,
      imapTls: false,
      smtpHost: "127.0.0.1",
      smtpPort: 1025,
      smtpTls: false,
    },
  },
  imap: {
    name: "Custom IMAP",
    loginHint: "手动填写 IMAP/SMTP 参数。适合自建邮箱或暂未内置的邮箱服务。",
    passwordLabel: "密码 / 授权码",
    values: {
      imapPort: 993,
      imapTls: true,
      smtpPort: 465,
      smtpTls: true,
    },
  },
}

function App() {
  const [, startTransition] =
    useTransition()

  const [dark, setDark] =
    useState(true)

  const [accounts, setAccounts] =
    useState<MailAccount[]>([])

  const [selectedAccountId, setSelectedAccountId] =
    useState<AccountFilterId>("all")

  const [showAddModal, setShowAddModal] =
    useState(false)

  const [showImapModal, setShowImapModal] =
    useState(false)

  const [showSettingsModal, setShowSettingsModal] =
    useState(false)

  const [showOutbox, setShowOutbox] =
    useState(false)

  const [showContacts, setShowContacts] =
    useState(false)

  const [editingAccount, setEditingAccount] =
    useState<MailAccount | null>(null)

  const [editingInitialValues, setEditingInitialValues] =
    useState<Partial<ImapAccountInput> | null>(null)

  const [sidebarAccountMode, setSidebarAccountMode] =
    useState<SidebarAccountMode>(() => {
      const saved = window.localStorage.getItem("woxmail.sidebarAccountMode")
      return saved === "list" ? "list" : "dropdown"
    })

  const [selectedProvider, setSelectedProvider] =
    useState<MailProvider>("imap")

  const [selectedFolder, setSelectedFolder] =
    useState<MailFolder>("INBOX")

  const [foldersByAccount, setFoldersByAccount] =
    useState<Record<string, MailFolderInfo[]>>({})

  const [unreadCounts, setUnreadCounts] =
    useState<UnreadCount[]>([])

  const [accountBusy, setAccountBusy] =
    useState(false)

  const [accountError, setAccountError] =
    useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      setAccountError(null)
      if (!isTauriRuntime()) {
        setAccounts([])
        return
      }

      try {
        // Single IPC call for accounts + unread counts
        const init = await batchInit()
        if (cancelled) return
        setAccounts(init.accounts)
        setUnreadCounts(init.unread_counts)

        // Load folders in background without blocking UI
        init.accounts.forEach((account) => {
          listFolders(account.id).then((folders) => {
            if (cancelled) return
            setFoldersByAccount((prev) => ({ ...prev, [account.id]: folders }))
          }).catch(() => {})
        })
      } catch (error) {
        if (!cancelled) {
          setAccountError(getErrorMessage(error))
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime() || accounts.length === 0) return

    let syncing = false
    let cancelled = false
    const run = async () => {
      if (syncing || cancelled) return
      syncing = true
      try {
        await syncInboxes()
        if (!cancelled) {
          await refreshUnreadCounts()
        }
      } catch {
        // Background inbox sync must stay quiet unless the user initiates it.
      } finally {
        syncing = false
      }
    }

    const startupTimer = window.setTimeout(() => void run(), 8_000)
    const interval = window.setInterval(() => void run(), inboxBackgroundSyncMs)

    return () => {
      cancelled = true
      window.clearTimeout(startupTimer)
      window.clearInterval(interval)
    }
  }, [accounts.length])

  useEffect(() => {
    if (!isTauriRuntime() || accounts.length === 0) return

    let running = false
    let cancelled = false
    const run = async () => {
      if (running || cancelled) return
      running = true
      try {
        await processOutbox()
      } catch {
        // Failed outbox jobs stay queued with backoff in the database.
      } finally {
        running = false
      }
    }

    const startupTimer = window.setTimeout(() => void run(), 3_000)
    const interval = window.setInterval(() => void run(), outboxBackgroundRetryMs)

    return () => {
      cancelled = true
      window.clearTimeout(startupTimer)
      window.clearInterval(interval)
    }
  }, [accounts.length])

  const addAccount = (
    provider: MailProvider,
    mode: AddAccountLoginMode = "custom",
  ) => {
    if (provider === "gmail" && mode === "oauth") {
      void addGmailOAuthAccount()
      return
    }
    if (provider === "outlook" && mode === "oauth") {
      void addOutlookOAuthAccount()
      return
    }

    setSelectedProvider(provider)
    setShowAddModal(false)
    setShowImapModal(true)
  }

  const addGmailOAuthAccount = async () => {
    setAccountBusy(true)
    setAccountError(null)
    try {
      const account = await gmailOAuthLogin()
      const remoteFolders = await listFolders(account.id)
      const firstFolder = findInitialFolder(remoteFolders)
      await syncFolder(account.id, firstFolder, false)

      setAccounts((prev) => [
        account,
        ...prev.filter((item) => item.id !== account.id),
      ])
      setFoldersByAccount((prev) => ({
        ...prev,
        [account.id]: remoteFolders,
      }))
      setSelectedFolder(firstFolder)
      setSelectedAccountId("all")
      setShowAddModal(false)
      await refreshUnreadCounts()
    } catch (error) {
      setAccountError(getErrorMessage(error))
    } finally {
      setAccountBusy(false)
    }
  }

  const addOutlookOAuthAccount = async () => {
    setAccountBusy(true)
    setAccountError(null)
    try {
      const account = await outlookOAuthLogin()
      const remoteFolders = await listFolders(account.id)
      const firstFolder = findInitialFolder(remoteFolders)
      await syncFolder(account.id, firstFolder, false)

      setAccounts((prev) => [
        account,
        ...prev.filter((item) => item.id !== account.id),
      ])
      setFoldersByAccount((prev) => ({
        ...prev,
        [account.id]: remoteFolders,
      }))
      setSelectedFolder(firstFolder)
      setSelectedAccountId("all")
      setShowAddModal(false)
      await refreshUnreadCounts()
    } catch (error) {
      setAccountError(getErrorMessage(error))
    } finally {
      setAccountBusy(false)
    }
  }

  const closeAddAccountModal = () => {
    setAccountBusy(false)
    setShowAddModal(false)
  }

  const editAccountLogin = (account: MailAccount) => {
    if (account.provider === "gmail") {
      void addGmailOAuthAccount()
      return
    }
    if (account.provider === "outlook") {
      void addOutlookOAuthAccount()
      return
    }

    void (async () => {
      setAccountBusy(true)
      setAccountError(null)
      try {
        const settings = await getAccountSettings(account.id)
        const preset = providerPresets[account.provider]?.values ?? providerPresets.imap.values
        setEditingAccount(account)
        setEditingInitialValues({
          ...preset,
          name: account.name,
          email: account.email,
          imapHost: settings?.imap_host ?? preset.imapHost,
          imapPort: settings?.imap_port ?? preset.imapPort,
          imapTls: settings?.imap_tls ?? preset.imapTls,
          imapUsername: settings?.imap_username ?? account.email,
          smtpHost: settings?.smtp_host ?? preset.smtpHost,
          smtpPort: settings?.smtp_port ?? preset.smtpPort,
          smtpTls: settings?.smtp_tls ?? preset.smtpTls,
          smtpUsername: settings?.smtp_username ?? account.email,
        })
      } catch (error) {
        setAccountError(getErrorMessage(error))
      } finally {
        setAccountBusy(false)
      }
    })()
  }

  const closeEditAccountLogin = () => {
    if (accountBusy) return
    setEditingAccount(null)
    setEditingInitialValues(null)
  }

  const addImapAccount = (input: ImapAccountInput) => {
    void (async () => {
      setAccountBusy(true)
      setAccountError(null)
      let createdAccountId: string | null = null
      try {
        const created = await createAccount({
          provider: selectedProvider,
          name: input.name.trim() ? input.name.trim() : providerPresets[selectedProvider].name,
          email: input.email.trim(),
        })
        createdAccountId = created.id

        await setAccountSettings({
          accountId: created.id,
          imapHost: input.imapHost.trim(),
          imapPort: input.imapPort,
          imapTls: input.imapTls,
          imapUsername: input.imapUsername.trim(),
          smtpHost: input.smtpHost.trim(),
          smtpPort: input.smtpPort,
          smtpTls: input.smtpTls,
          smtpUsername: input.smtpUsername.trim(),
          password: input.password,
        })

        const remoteFolders = await listFolders(created.id)
        const firstFolder = findInitialFolder(remoteFolders)
        await syncFolder(created.id, firstFolder, false)

        setAccounts((prev) => [
          created,
          ...prev,
        ])
        setFoldersByAccount((prev) => ({
          ...prev,
          [created.id]: remoteFolders,
        }))
        setSelectedFolder(firstFolder)
        setSelectedAccountId("all")
        setShowImapModal(false)
        await refreshUnreadCounts()
      } catch (error) {
        if (createdAccountId) {
          await deleteAccount(createdAccountId).catch(() => undefined)
        }
        setAccountError(getErrorMessage(error))
      } finally {
        setAccountBusy(false)
      }
    })()
  }

  const updateAccountLogin = (input: ImapAccountInput) => {
    if (!editingAccount) return

    void (async () => {
      setAccountBusy(true)
      setAccountError(null)
      try {
        await setAccountSettings({
          accountId: editingAccount.id,
          imapHost: input.imapHost.trim(),
          imapPort: input.imapPort,
          imapTls: input.imapTls,
          imapUsername: input.imapUsername.trim(),
          smtpHost: input.smtpHost.trim(),
          smtpPort: input.smtpPort,
          smtpTls: input.smtpTls,
          smtpUsername: input.smtpUsername.trim(),
          password: input.password,
        })

        const remoteFolders = await listFolders(editingAccount.id)
        const firstFolder = findInitialFolder(remoteFolders)
        await syncFolder(editingAccount.id, firstFolder, false)

        setFoldersByAccount((prev) => ({
          ...prev,
          [editingAccount.id]: remoteFolders,
        }))
        setSelectedAccountId(editingAccount.id)
        setSelectedFolder(firstFolder)
        setEditingAccount(null)
        setEditingInitialValues(null)
        await refreshUnreadCounts()
      } catch (error) {
        setAccountError(getErrorMessage(error))
      } finally {
        setAccountBusy(false)
      }
    })()
  }

  const removeAccount = (account: MailAccount) => {
    if (!window.confirm(`删除账户 ${account.email}？本地缓存的邮件也会一起删除。`)) {
      return
    }

    void (async () => {
      setAccountBusy(true)
      setAccountError(null)
      try {
        await deleteAccount(account.id)
        setFoldersByAccount((prev) => {
          const next = { ...prev }
          delete next[account.id]
          return next
        })
        setAccounts((prev) => {
          const next = prev.filter((item) => item.id !== account.id)
          setSelectedAccountId((current) => {
            if (current !== account.id) return current
            return "all"
          })
          return next
        })
        await refreshUnreadCounts()
      } catch (error) {
        setAccountError(getErrorMessage(error))
      } finally {
        setAccountBusy(false)
      }
    })()
  }

  const updateSidebarAccountMode = (mode: SidebarAccountMode) => {
    setSidebarAccountMode(mode)
    window.localStorage.setItem("woxmail.sidebarAccountMode", mode)
  }

  const refreshUnreadCounts = async () => {
    if (!isTauriRuntime()) return
    try {
      setUnreadCounts(await listUnreadCounts())
    } catch {
      // Badge refresh should never block mail workflows.
    }
  }

  const selectAccount = (accountId: AccountFilterId) => {
    setShowContacts(false)
    setShowOutbox(false)
    startTransition(() => {
      setSelectedAccountId(accountId)
    })
  }

  const selectFolder = (folder: MailFolder) => {
    setShowContacts(false)
    setShowOutbox(false)
    startTransition(() => {
      setSelectedFolder(folder)
    })
  }

  useKeyboardShortcuts((action: ShortcutAction) => {
    switch (action) {
      case "compose":
        // Trigger compose via MailScreen internal state — handled by component
        break
      case "refresh":
        void refreshUnreadCounts()
        break
    }
  })

  const visibleFolders = useMemo(
    () => mergeVisibleFolders(accounts, foldersByAccount, selectedAccountId),
    [accounts, foldersByAccount, selectedAccountId],
  )

  const unreadBadges = useMemo(
    () => buildUnreadBadges(unreadCounts, accounts, selectedAccountId),
    [accounts, selectedAccountId, unreadCounts],
  )

  useEffect(() => {
    if (visibleFolders.length === 0) return
    if (visibleFolders.some((folder) => folder.path === selectedFolder)) return
    setSelectedFolder(findInitialFolder(visibleFolders))
  }, [selectedFolder, visibleFolders])

  return (
    <div
      className={`flex h-screen flex-col ${dark
          ? "bg-[#09090B] text-white"
          : "bg-zinc-100 text-black"
        }`}
    >
      <TitleBar
        dark={dark}
        setDark={setDark}
        onSettings={() =>
          setShowSettingsModal(true)
        }
      />

      <div className="flex flex-1 overflow-hidden">
        <Sidebar
          dark={dark}
          accounts={accounts}
          selectedAccountId={selectedAccountId}
          accountMode={sidebarAccountMode}
          folders={visibleFolders}
          accountUnreadCounts={unreadBadges.byAccount}
          folderUnreadCounts={unreadBadges.byFolder}
          totalUnreadCount={unreadBadges.total}
          selectedFolder={selectedFolder}
          onSelectAccount={selectAccount}
          onSelectFolder={selectFolder}
          onAdd={() =>
            setShowAddModal(true)
          }
          onEditAccount={editAccountLogin}
          onDeleteAccount={removeAccount}
          onOpenOutbox={() => { setShowContacts(false); setShowOutbox(true) }}
          onOpenContacts={() => { setShowOutbox(false); setShowContacts(true) }}
          error={accountError}
        />

        {showContacts ? (
          <ContactScreen dark={dark} />
        ) : showOutbox ? (
          <OutboxScreen dark={dark} />
        ) : (
          <MailScreen
            dark={dark}
            accounts={accounts}
            accountFilterId={selectedAccountId}
            onAccountFilterChange={selectAccount}
            folder={selectedFolder}
            foldersByAccount={foldersByAccount}
            onFolderChange={selectFolder}
            onUnreadCountsChanged={refreshUnreadCounts}
          />
        )}
      </div>

      {showAddModal && (
        <AddAccountModal
          dark={dark}
          busy={accountBusy}
          error={accountError}
          onClose={closeAddAccountModal}
          onSelect={addAccount}
        />
      )}

      {showImapModal && (
        <AddImapAccountModal
          dark={dark}
          busy={accountBusy}
          error={accountError}
          providerName={providerPresets[selectedProvider].name}
          initialValues={providerPresets[selectedProvider].values}
          loginHint={providerPresets[selectedProvider].loginHint}
          passwordLabel={providerPresets[selectedProvider].passwordLabel}
          onClose={() =>
            setShowImapModal(false)
          }
          onSubmit={addImapAccount}
        />
      )}

      {editingAccount && editingInitialValues && (
        <AddImapAccountModal
          dark={dark}
          busy={accountBusy}
          error={accountError}
          providerName={`更新登录：${editingAccount.email}`}
          initialValues={editingInitialValues}
          loginHint="重新保存 IMAP/SMTP 密码或授权码。服务器参数会保留，可按需调整。"
          passwordLabel={providerPresets[editingAccount.provider]?.passwordLabel ?? "密码 / 授权码"}
          onClose={closeEditAccountLogin}
          onSubmit={updateAccountLogin}
        />
      )}

      {showSettingsModal && (
        <SettingsModal
          dark={dark}
          sidebarAccountMode={sidebarAccountMode}
          onSidebarAccountModeChange={updateSidebarAccountMode}
          onClose={() =>
            setShowSettingsModal(false)
          }
        />
      )}
    </div>
  )
}

function getErrorMessage(error: unknown) {
  return error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "操作失败，请稍后重试"
}

function findInitialFolder(folders: MailFolderInfo[]) {
  const selectable = folders.filter(isSelectableFolder)
  return (
    selectable.find((folder) => folder.path.toLowerCase() === "inbox") ??
    selectable[0] ??
    folders[0]
  )?.path ?? "INBOX"
}

function mergeVisibleFolders(
  accounts: MailAccount[],
  foldersByAccount: Record<string, MailFolderInfo[]>,
  selectedAccountId: AccountFilterId,
) {
  const selectedAccounts =
    selectedAccountId === "all"
      ? accounts
      : accounts.filter((account) => account.id === selectedAccountId)

  const byPath = new Map<string, MailFolderInfo>()
  for (const account of selectedAccounts) {
    for (const folder of foldersByAccount[account.id] ?? []) {
      if (!isSelectableFolder(folder)) continue

      const current = byPath.get(folder.path)
      if (current) {
        byPath.set(folder.path, {
          ...current,
          selectable: true,
        })
      } else {
        byPath.set(folder.path, folder)
      }
    }
  }

  return Array.from(byPath.values()).sort((a, b) => {
    const aInbox = a.path.toLowerCase() === "inbox"
    const bInbox = b.path.toLowerCase() === "inbox"
    if (aInbox !== bInbox) return aInbox ? -1 : 1
    return a.name.localeCompare(b.name)
  })
}

function buildUnreadBadges(
  unreadCounts: UnreadCount[],
  accounts: MailAccount[],
  selectedAccountId: AccountFilterId,
) {
  const visibleAccountIds = new Set(
    selectedAccountId === "all"
      ? accounts.map((account) => account.id)
      : accounts.filter((account) => account.id === selectedAccountId).map((account) => account.id),
  )
  const byAccount: Record<string, number> = {}
  const byFolder: Record<string, number> = {}
  let total = 0

  for (const item of unreadCounts) {
    const count = item.unread_count
    byAccount[item.account_id] = (byAccount[item.account_id] ?? 0) + count
    if (visibleAccountIds.has(item.account_id)) {
      byFolder[item.folder_path] = (byFolder[item.folder_path] ?? 0) + count
      total += count
    }
  }

  return {
    byAccount,
    byFolder,
    total,
  }
}

export default App
