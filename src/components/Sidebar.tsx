import type { ReactNode } from "react"
import { Folder, KeyRound, Mail, Plus, Send, Trash2, UsersRound } from "lucide-react" 
import { useTranslation } from "react-i18next"
import type { MailAccount, MailFolder, MailFolderInfo } from "../types/mail"
import { folderDisplayName, isSelectableFolder } from "../utils/folders"

type Props = {
  dark: boolean
  accounts: MailAccount[]
  selectedAccountId: "all" | string
  accountMode: "list" | "dropdown"
  folders: MailFolderInfo[]
  accountUnreadCounts: Record<string, number>
  folderUnreadCounts: Record<string, number>
  totalUnreadCount: number
  selectedFolder: MailFolder
  error?: string | null
  onSelectAccount: (accountId: "all" | string) => void
  onSelectFolder: (folder: MailFolder) => void
  onAdd: () => void
  onEditAccount: (account: MailAccount) => void
  onDeleteAccount: (account: MailAccount) => void
  onOpenOutbox: () => void
  onOpenContacts: () => void
}

function Sidebar({
  dark,
  accounts,
  selectedAccountId,
  accountMode,
  folders,
  accountUnreadCounts,
  folderUnreadCounts,
  totalUnreadCount,
  selectedFolder,
  error,
  onSelectAccount,
  onSelectFolder,
  onAdd,
  onEditAccount,
  onDeleteAccount,
  onOpenOutbox,
  onOpenContacts,
}: Props) {
  const { t } = useTranslation()
  const selectedAccount = accounts.find((account) => account.id === selectedAccountId) ?? null
  const selectableFolders = folders.filter(isSelectableFolder)
  const selectedAccountUnread = selectedAccount ? accountUnreadCounts[selectedAccount.id] ?? 0 : 0

  return (
    <div className={`flex h-full w-72 flex-col border-r ${dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"}`}>
      <div className={`shrink-0 border-b p-6 ${dark ? "border-white/10" : "border-black/10"}`}>
        <div className="flex items-center gap-3">
          <div className="rounded-2xl bg-blue-500/20 p-3">
            <Mail size={24} />
          </div>
          <div>
            <div className="text-xl font-semibold">Wox Mail</div>
            <div className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>Modern Mail Client</div>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        <div className={`mb-3 text-sm ${dark ? "text-zinc-500" : "text-zinc-600"}`}>
          {t("accounts")}
        </div>

        {error && (
          <div className="mb-3 rounded-2xl border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-300">
            {error}
          </div>
        )}

        {accounts.length === 0 ? (
          <div className={`rounded-2xl border border-dashed p-6 text-center ${dark ? "border-white/10 text-zinc-500" : "border-black/10 text-zinc-600"}`}>
            {t("no_accounts")}
          </div>
        ) : accountMode === "dropdown" ? (
          <>
            <select
              value={selectedAccountId}
              onChange={(event) => onSelectAccount(event.target.value)}
              className={`mb-3 w-full rounded-2xl border px-3 py-3 text-sm outline-none ${
                dark
                  ? "border-white/10 bg-[#151518] text-white"
                  : "border-black/10 bg-white text-black"
              }`}
            >
              <option value="all">全部邮件</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>
                  {account.email}
                </option>
              ))}
            </select>
            {selectedAccount && (
              <div className={`mb-3 flex items-center gap-3 rounded-2xl border p-3 ${
                dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"
              }`}>
                <IconWithBadge count={selectedAccountUnread} dark={dark}>
                  <Mail size={18} />
                </IconWithBadge>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">{selectedAccount.name}</div>
                  <div className={`truncate text-xs ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                    {selectedAccount.email}
                  </div>
                </div>
              </div>
            )}
            {selectedAccount && (
              <div className="mb-3 grid grid-cols-2 gap-2">
                <button
                  onClick={() => onEditAccount(selectedAccount)}
                  className={`flex items-center justify-center gap-2 rounded-xl px-3 py-2 text-xs transition ${
                    dark ? "bg-white/10 text-zinc-200 hover:bg-white/15" : "bg-black/10 text-zinc-700 hover:bg-black/15"
                  }`}
                >
                  <KeyRound size={14} />
                  更新登录
                </button>
                <button
                  onClick={() => onDeleteAccount(selectedAccount)}
                  className={`flex items-center justify-center gap-2 rounded-xl px-3 py-2 text-xs transition ${
                    dark ? "bg-red-500/10 text-red-300 hover:bg-red-500/15" : "bg-red-500/10 text-red-600 hover:bg-red-500/15"
                  }`}
                >
                  <Trash2 size={14} />
                  删除
                </button>
              </div>
            )}
          </>
        ) : (
          <>
            <button
              onClick={() => onSelectAccount("all")}
              className={`mb-3 w-full rounded-2xl border p-3 text-left transition ${dark ? "border-white/10 bg-white/5 hover:bg-white/10" : "border-black/10 bg-black/5 hover:bg-black/10"} ${
                selectedAccountId === "all" ? (dark ? "ring-1 ring-white/20" : "ring-1 ring-black/20") : ""
              }`}
            >
              <div className="flex items-center gap-3">
                <IconWithBadge count={totalUnreadCount} dark={dark}>
                  <Mail size={18} />
                </IconWithBadge>
                <div className="min-w-0">
                  <div className="font-medium">全部邮件</div>
                  <div className={`mt-1 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>所有邮箱账号</div>
                </div>
              </div>
            </button>
            {accounts.map((account) => (
              <div
                key={account.id}
                className={`mb-3 flex items-center gap-2 rounded-2xl border p-3 transition ${dark ? "border-white/10 bg-white/5 hover:bg-white/10" : "border-black/10 bg-black/5 hover:bg-black/10"} ${
                  account.id === selectedAccountId ? (dark ? "ring-1 ring-white/20" : "ring-1 ring-black/20") : ""
                }`}
              >
                <button
                  onClick={() => onSelectAccount(account.id)}
                  className="flex min-w-0 flex-1 items-center gap-3 text-left"
                >
                  <IconWithBadge count={accountUnreadCounts[account.id] ?? 0} dark={dark}>
                    <Mail size={16} />
                  </IconWithBadge>
                  <div className="min-w-0">
                    <div className="truncate font-medium">{account.name}</div>
                    <div className={`mt-1 truncate text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>{account.email}</div>
                  </div>
                </button>
                <button
                  title="更新登录信息"
                  onClick={() => onEditAccount(account)}
                  className={`shrink-0 rounded-xl p-2 transition ${
                    dark ? "text-zinc-500 hover:bg-white/10 hover:text-zinc-200" : "text-zinc-500 hover:bg-black/10 hover:text-zinc-700"
                  }`}
                >
                  <KeyRound size={16} />
                </button>
                <button
                  title="删除账户"
                  onClick={() => onDeleteAccount(account)}
                  className={`shrink-0 rounded-xl p-2 transition ${
                    dark ? "text-zinc-500 hover:bg-red-500/10 hover:text-red-300" : "text-zinc-500 hover:bg-red-500/10 hover:text-red-600"
                  }`}
                >
                  <Trash2 size={16} />
                </button>
              </div>
            ))}
          </>
        )}

        <div className={`mb-3 mt-5 text-sm ${dark ? "text-zinc-500" : "text-zinc-600"}`}>
          {t("folders")}
        </div>
        {selectableFolders.length === 0 ? (
          <div className={`rounded-2xl border border-dashed p-4 text-sm ${dark ? "border-white/10 text-zinc-500" : "border-black/10 text-zinc-600"}`}>
            登录后显示邮箱服务器文件夹
          </div>
        ) : selectableFolders.map((folder) => (
          <button
            key={`${folder.account_id}:${folder.path}`}
            onClick={() => onSelectFolder(folder.path)}
            className={`mb-2 flex w-full items-center gap-2 rounded-2xl px-3 py-2 text-left text-sm transition ${
              selectedFolder === folder.path
                ? dark
                  ? "bg-white/10 text-white"
                  : "bg-black/10 text-black"
                : dark
                  ? "text-zinc-400 hover:bg-white/5"
                  : "text-zinc-600 hover:bg-black/5"
            }`}
          >
            <IconWithBadge count={folderUnreadCounts[folder.path] ?? 0} dark={dark}>
              <Folder size={16} />
            </IconWithBadge>
            <span className="truncate">{folderDisplayName(folder, t)}</span>
          </button>
        ))}

      </div>
      <div className={`shrink-0 border-t p-4 ${dark ? "border-white/10" : "border-black/10"}`}>
        {accounts.length > 0 && (
          <>
            <button
              onClick={onOpenContacts}
              className={`mb-2 flex w-full items-center justify-center gap-2 rounded-2xl p-4 transition ${
                dark ? "bg-white/5 text-zinc-200 hover:bg-white/10" : "bg-black/5 text-zinc-700 hover:bg-black/10"
              }`}
            >
              <UsersRound size={18} />
              通讯录
            </button>
            <button
              onClick={onOpenOutbox}
              className={`mb-3 flex w-full items-center justify-center gap-2 rounded-2xl p-4 transition ${
                dark ? "bg-white/5 text-zinc-200 hover:bg-white/10" : "bg-black/5 text-zinc-700 hover:bg-black/10"
              }`}
            >
              <Send size={18} />
              发件箱
            </button>
          </>
        )}
        <button
          onClick={onAdd}
          className={`flex w-full items-center justify-center gap-2 rounded-2xl border border-dashed p-4 transition ${dark ? "border-white/10 hover:bg-white/5" : "border-black/10 hover:bg-black/5"}`}
        >
          <Plus size={18} />
          {t("add_account")}
        </button>
      </div>
    </div>
  )
}

function IconWithBadge({
  children,
  count,
  dark,
}: {
  children: ReactNode
  count: number
  dark: boolean
}) {
  return (
    <span className={`relative inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-xl ${
      dark ? "bg-white/10" : "bg-black/10"
    }`}>
      {children}
      {count > 0 && (
        <span className={`absolute -right-2 -top-2 flex min-h-5 min-w-5 items-center justify-center rounded-full bg-red-500 px-1 text-[10px] font-bold leading-none text-white ring-2 ${
          dark ? "ring-[#111]" : "ring-white"
        }`}>
          {count > 99 ? "99+" : count}
        </span>
      )}
    </span>
  )
}

export default Sidebar
