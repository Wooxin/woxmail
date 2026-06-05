import { useEffect, useMemo, useRef, useState } from "react"
import { Paperclip, Plus, RefreshCcw } from "lucide-react"
import { useTranslation } from "react-i18next"

import type { MailAccount, MailFolder, MailFolderInfo, MessageDetail, MessageSummary } from "../types/mail"
import {
  addMessageTag,
  clearMessageTags,
  getMessage,
  listMessages,
  markMessageRead,
  moveMessagesToFolder,
  sendMessage,
  syncFolder,
} from "../api/mail"
import { folderDisplayName } from "../utils/folders"

type Props = {
  dark: boolean
  accounts: MailAccount[]
  accountFilterId: "all" | string
  onAccountFilterChange: (accountId: "all" | string) => void
  folder: MailFolder
  foldersByAccount: Record<string, MailFolderInfo[]>
  onFolderChange: (folder: MailFolder) => void
  onUnreadCountsChanged: () => void | Promise<void>
}

type MessageGroup = {
  id: string
  latest: MessageSummary
  messages: MessageSummary[]
  unreadCount: number
  attachmentCount: number
}

const pageSize = 50
const backgroundSyncMs = 90_000
const syncCooldownMs = 30_000

function formatTs(ts: number) {
  const d = new Date(ts * 1000)
  return d.toLocaleString()
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B"

  const units = ["B", "KB", "MB", "GB"]
  let size = bytes
  let unitIndex = 0
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex += 1
  }

  return `${size >= 10 || unitIndex === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unitIndex]}`
}

function MailScreen({
  dark,
  accounts,
  accountFilterId,
  onAccountFilterChange,
  folder,
  foldersByAccount,
  onFolderChange,
  onUnreadCountsChanged,
}: Props) {
  const { t } = useTranslation()
  const [loading, setLoading] = useState(false)
  const [syncing, setSyncing] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [detailLoading, setDetailLoading] = useState(false)
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [messages, setMessages] = useState<MessageSummary[]>([])
  const [hasMore, setHasMore] = useState(false)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<MessageDetail | null>(null)
  const [showCompose, setShowCompose] = useState(false)
  const [contextMenu, setContextMenu] = useState<{
    x: number
    y: number
    group: MessageGroup
  } | null>(null)
  const lastSyncAtRef = useRef<Record<string, number>>({})

  const [fromAccountId, setFromAccountId] = useState(accounts[0]?.id ?? "")
  const [toInput, setToInput] = useState("")
  const [subjectInput, setSubjectInput] = useState("")
  const [bodyInput, setBodyInput] = useState("")

  const selectedSummary = useMemo(
    () => messages.find((m) => m.id === selectedId) ?? null,
    [messages, selectedId],
  )

  const messageGroups = useMemo(
    () => groupMessagesByContact(messages, isSentFolder(folder)),
    [folder, messages],
  )

  const activeAccounts = useMemo(
    () => {
      const selectedAccounts =
        accountFilterId === "all"
          ? accounts
          : accounts.filter((account) => account.id === accountFilterId)
      const accountsWithFolder = selectedAccounts.filter((account) =>
        (foldersByAccount[account.id] ?? []).some((item) => item.path === folder && item.selectable),
      )
      return accountsWithFolder.length > 0 ? accountsWithFolder : selectedAccounts
    },
    [accountFilterId, accounts, folder, foldersByAccount],
  )

  const folderTitle = useMemo(
    () => {
      const folderInfo = findFolderInfo(folder, foldersByAccount)
      return folderDisplayName(folderInfo ?? folder, t)
    },
    [folder, foldersByAccount, t],
  )

  useEffect(() => {
    if (!accounts.some((account) => account.id === fromAccountId)) {
      setFromAccountId(accounts[0]?.id ?? "")
    }
  }, [accounts, fromAccountId])

  const loadMessages = async (
    nextFolder: MailFolder = folder,
    preferredMessageId?: string,
    options?: {
      append?: boolean
      background?: boolean
      skipSync?: boolean
      forceSync?: boolean
    },
  ) => {
    const append = options?.append ?? false
    const background = options?.background ?? false
    const targetAccounts = getActiveAccountsForFolder(
      accounts,
      accountFilterId,
      foldersByAccount,
      nextFolder,
    )
    const canPage = targetAccounts.length === 1
    const offset = append && canPage ? messages.length : 0

    if (append) {
      setLoadingMore(true)
    } else if (background) {
      setSyncing(true)
    } else {
      setLoading(true)
    }

    if (!background) setError(null)
    try {
      const accountFolders = new Map(
        targetAccounts.map((account) => [
          account.id,
          resolveFolderForAccount(account.id, foldersByAccount, nextFolder),
        ]),
      )

      const readMessages = async () => {
        const lists = await Promise.all(
          targetAccounts.map((account) =>
            listMessages({
              accountId: account.id,
              folderPath: accountFolders.get(account.id) ?? nextFolder,
              limit: pageSize,
              offset,
            }),
          ),
        )
        return lists.flat().sort((a, b) => b.date_ts - a.date_ts)
      }

      const applyMessages = (list: MessageSummary[]) => {
        const nextMessages = append
          ? mergeMessages(messages, list)
          : background
            ? mergeMessages(list, messages)
            : list
        setMessages(nextMessages)
        setHasMore((current) => (background ? current : canPage && list.length === pageSize))
        setSelectedId((current) =>
          preferredMessageId && nextMessages.some((message) => message.id === preferredMessageId)
            ? preferredMessageId
            : current && nextMessages.some((message) => message.id === current)
              ? current
              : nextMessages[0]?.id ?? null,
        )
      }

      if (!append && !background) {
        applyMessages(await readMessages())
        setLoading(false)
      }

      if (!options?.skipSync && !append) {
        const now = Date.now()
        const accountsToSync = targetAccounts.filter((account) => {
          const accountFolder = accountFolders.get(account.id) ?? nextFolder
          const key = `${account.id}:${accountFolder}`
          return options?.forceSync || now - (lastSyncAtRef.current[key] ?? 0) > syncCooldownMs
        })

        let syncedNewMessages = 0
        await Promise.all(accountsToSync.map(async (account) => {
          const accountFolder = accountFolders.get(account.id) ?? nextFolder
          const inserted = await syncFolder(account.id, accountFolder)
          syncedNewMessages += inserted
          lastSyncAtRef.current[`${account.id}:${accountFolder}`] = Date.now()
        }))
        if (syncedNewMessages > 0) {
          void onUnreadCountsChanged()
        }
      }

      applyMessages(await readMessages())
    } catch (loadError) {
      if (!background) setError(getErrorMessage(loadError))
    } finally {
      setLoading(false)
      setSyncing(false)
      setLoadingMore(false)
    }
  }

  useEffect(() => {
    setMessages([])
    setSelectedId(null)
    setDetail(null)
    setHasMore(false)
    void loadMessages(folder)
  }, [accountFilterId, accounts.length])

  useEffect(() => {
    setMessages([])
    setSelectedId(null)
    setDetail(null)
    setHasMore(false)
    void loadMessages(folder)
  }, [folder])

  useEffect(() => {
    const id = window.setInterval(() => {
      if (loading || syncing || loadingMore || sending) return
      void loadMessages(folder, selectedId ?? undefined, { background: true })
    }, backgroundSyncMs)

    return () => window.clearInterval(id)
  }, [accountFilterId, accounts.length, folder, loading, syncing, loadingMore, sending, selectedId, messages.length])

  useEffect(() => {
    if (!selectedId) {
      setDetail(null)
      return
    }
    let cancelled = false
    void (async () => {
      setDetailLoading(true)
      setError(null)
      try {
        const msg = await getMessage(selectedId)
        if (cancelled) return
        setDetail(msg)

        if (!msg.is_read) {
          await markMessageRead(msg.id)
          if (cancelled) return
          setDetail({ ...msg, is_read: true })
          setMessages((current) =>
            current.map((message) =>
              message.id === msg.id ? { ...message, is_read: true } : message,
            ),
          )
          void onUnreadCountsChanged()
        }
      } catch (detailError) {
        if (!cancelled) {
          setError(getErrorMessage(detailError))
        }
      } finally {
        if (!cancelled) setDetailLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [selectedId])

  const onSend = async () => {
    const to = toInput
      .split(/[,\s]+/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (to.length === 0) return

    setSending(true)
    setError(null)
    try {
      const sendAccountId = fromAccountId || accounts[0]?.id
      if (!sendAccountId) {
        throw new Error("请先添加发件账户")
      }

      const sentFolder = findSentFolder(sendAccountId, foldersByAccount) ?? "Sent"
      const sentMessageId = await sendMessage({
        account_id: sendAccountId,
        to_emails: to,
        subject: subjectInput.trim(),
        body: bodyInput,
        sent_folder_path: sentFolder,
      })

      setShowCompose(false)
      setToInput("")
      setSubjectInput("")
      setBodyInput("")
      onFolderChange(sentFolder)
      if (accountFilterId !== "all" && accountFilterId !== sendAccountId) {
        onAccountFilterChange(sendAccountId)
      }
      await loadMessages(sentFolder, sentMessageId, { skipSync: true })
    } catch (sendError) {
      setError(getErrorMessage(sendError))
    } finally {
      setSending(false)
    }
  }

  const canSend = toInput.trim().length > 0 && !sending
  const loadMore = () => {
    void loadMessages(folder, selectedId ?? undefined, { append: true, skipSync: true })
  }

  const closeContextMenu = () => setContextMenu(null)

  const tagGroup = async (group: MessageGroup, tag: string) => {
    closeContextMenu()
    const ids = group.messages.map((message) => message.id)
    setError(null)
    try {
      await addMessageTag(ids, tag)
      setMessages((current) =>
        current.map((message) =>
          ids.includes(message.id) && !message.tags.includes(tag)
            ? { ...message, tags: [...message.tags, tag] }
            : message,
        ),
      )
    } catch (tagError) {
      setError(getErrorMessage(tagError))
    }
  }

  const clearTags = async (group: MessageGroup) => {
    closeContextMenu()
    const ids = group.messages.map((message) => message.id)
    setError(null)
    try {
      await clearMessageTags(ids)
      setMessages((current) =>
        current.map((message) =>
          ids.includes(message.id) ? { ...message, tags: [] } : message,
        ),
      )
    } catch (tagError) {
      setError(getErrorMessage(tagError))
    }
  }

  const moveGroup = async (group: MessageGroup, targetFolder: string) => {
    closeContextMenu()
    const ids = group.messages.map((message) => message.id)
    setError(null)
    try {
      await moveMessagesToFolder(ids, targetFolder)
      setMessages((current) => current.filter((message) => !ids.includes(message.id)))
      setSelectedId((current) => current && ids.includes(current) ? null : current)
      void onUnreadCountsChanged()
    } catch (moveError) {
      setError(getErrorMessage(moveError))
    }
  }

  const archiveFolder = useMemo(
    () => findActionFolder(foldersByAccount, ["all mail", "archive", "归档", "所有邮件"]) ?? "Archive",
    [foldersByAccount],
  )

  const junkFolder = useMemo(
    () => findActionFolder(foldersByAccount, ["spam", "junk", "垃圾", "垃圾邮件"]) ?? "Junk",
    [foldersByAccount],
  )

  return (
    <div
      className="relative flex flex-1 flex-col overflow-hidden"
      onClick={contextMenu ? closeContextMenu : undefined}
    >
      <div
        className={`flex items-center justify-between border-b px-6 py-4 ${
          dark ? "border-white/10" : "border-black/10"
        }`}
      >
        <div>
          <div className="text-lg font-semibold">{folderTitle}</div>
          <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} text-sm`}>
            {loading ? "正在加载..." : syncing ? "后台同步中..." : accountFilterId === "all" ? "全部邮箱" : activeAccounts[0]?.email}
          </div>
        </div>

        <div className="flex items-center gap-2">
          <select
            value={accountFilterId}
            onChange={(event) => onAccountFilterChange(event.target.value)}
            className={`rounded-xl border px-3 py-2 text-sm outline-none ${
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
          <button
            disabled={loading}
            onClick={() => void loadMessages(folder, selectedId ?? undefined, { forceSync: true })}
            className={`flex items-center gap-2 rounded-xl px-3 py-2 text-sm transition ${
              dark ? "hover:bg-white/10" : "hover:bg-black/10"
            }`}
          >
            <RefreshCcw size={16} />
            {loading ? "刷新中" : "刷新"}
          </button>
          <button
            onClick={() => setShowCompose(true)}
            className="flex items-center gap-2 rounded-xl bg-white px-3 py-2 text-sm font-medium text-black transition hover:scale-[1.02]"
          >
            <Plus size={16} />
            写信
          </button>
        </div>
      </div>

      {error && (
        <div className="flex items-center justify-between gap-3 border-b border-red-500/20 bg-red-500/10 px-6 py-3 text-sm text-red-300">
          <span>{error}</span>
          <button
            disabled={loading || syncing}
            onClick={() => void loadMessages(folder, selectedId ?? undefined)}
            className="rounded-lg px-2 py-1 text-xs font-medium transition hover:bg-red-500/10 disabled:opacity-50"
          >
            重试
          </button>
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        <div
          className={`w-[420px] border-r ${
            dark ? "border-white/10" : "border-black/10"
          } overflow-y-auto`}
        >
          {loading && messages.length === 0 ? (
            <div className={`p-6 ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              正在加载邮件...
            </div>
          ) : messages.length === 0 ? (
            <div className={`p-6 ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              暂无邮件
            </div>
          ) : (
            <div className="p-3">
              {messageGroups.map((group) => {
                const m = group.latest
                const selected = group.messages.some((message) => message.id === selectedId)

                return (
                <button
                  key={group.id}
                  onContextMenu={(event) => {
                    event.preventDefault()
                    setContextMenu({
                      x: event.clientX,
                      y: event.clientY,
                      group,
                    })
                  }}
                  onClick={() => setSelectedId(m.id)}
                  className={`mb-2 w-full rounded-2xl border p-4 text-left transition ${
                    dark
                      ? "border-white/10 hover:bg-white/5"
                      : "border-black/10 hover:bg-black/5"
                  } ${
                    selected
                      ? dark
                        ? "bg-white/10"
                        : "bg-black/10"
                      : ""
                  }`}
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex min-w-0 items-center gap-2">
                      {group.unreadCount > 0 && (
                        <span className="h-2 w-2 shrink-0 rounded-full bg-sky-400" />
                      )}
                      <div className={`truncate ${group.unreadCount === 0 ? "font-medium" : "font-semibold"}`}>
                        {m.subject}
                      </div>
                      {group.attachmentCount > 0 && (
                        <span
                          className={`flex shrink-0 items-center gap-1 text-xs ${
                            dark ? "text-zinc-500" : "text-zinc-600"
                          }`}
                        >
                          <Paperclip size={12} />
                          {group.attachmentCount}
                        </span>
                      )}
                    </div>
                    <div className={`${dark ? "text-zinc-500" : "text-zinc-600"} shrink-0 text-xs`}>
                      {group.messages.length > 1 ? `${group.messages.length} 封` : formatTs(m.date_ts)}
                    </div>
                  </div>
                  <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} mt-1 truncate text-sm`}>
                    {isSentFolder(folder)
                      ? `To: ${m.to_emails.join(", ")}`
                      : `${m.from_name} <${m.from_email}>`}
                  </div>
                  {m.tags.length > 0 && (
                    <div className="mt-2 flex flex-wrap gap-1">
                      {m.tags.map((tag) => (
                        <span
                          key={tag}
                          className={`rounded-full px-2 py-0.5 text-xs ${
                            dark ? "bg-sky-400/15 text-sky-200" : "bg-sky-500/10 text-sky-700"
                          }`}
                        >
                          {tag}
                        </span>
                      ))}
                    </div>
                  )}
                  <div
                    className={`${dark ? "text-zinc-400" : "text-zinc-600"} mt-2 overflow-hidden text-sm`}
                    style={{
                      display: "-webkit-box",
                      WebkitLineClamp: 2,
                      WebkitBoxOrient: "vertical",
                    }}
                  >
                    {m.snippet}
                  </div>
                </button>
                )
              })}

              {hasMore && (
                <button
                  disabled={loadingMore}
                  onClick={loadMore}
                  className={`mt-2 w-full rounded-xl border px-3 py-2 text-sm transition ${
                    dark
                      ? "border-white/10 text-zinc-300 hover:bg-white/5"
                      : "border-black/10 text-zinc-700 hover:bg-black/5"
                  } disabled:opacity-50`}
                >
                  {loadingMore ? "加载中..." : "加载更多"}
                </button>
              )}
            </div>
          )}
        </div>

        <div className="flex-1 overflow-y-auto p-8">
          {!detail ? (
            <div className={`${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              {selectedSummary || detailLoading ? "加载中..." : "请选择一封邮件"}
            </div>
          ) : (
            <div>
              <div className="text-2xl font-bold">{detail.subject}</div>
              <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} mt-3 text-sm`}>
                From: {detail.from_name} &lt;{detail.from_email}&gt;
              </div>
              <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} mt-1 text-sm`}>
                To: {detail.to_emails.join(", ")}
              </div>
              <div className={`${dark ? "text-zinc-500" : "text-zinc-600"} mt-1 text-sm`}>
                Date: {formatTs(detail.date_ts)}
              </div>

              {detail.attachments.length > 0 && (
                <div
                  className={`mt-5 rounded-2xl border p-4 ${
                    dark ? "border-white/10 bg-white/[0.03]" : "border-black/10 bg-black/[0.03]"
                  }`}
                >
                  <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
                    <Paperclip size={16} />
                    附件
                  </div>
                  <div className="grid gap-2">
                    {detail.attachments.map((attachment) => (
                      <div
                        key={attachment.id}
                        className={`flex items-center justify-between gap-3 rounded-xl border px-3 py-2 text-sm ${
                          dark ? "border-white/10 bg-black/10" : "border-black/10 bg-white"
                        }`}
                      >
                        <div className="min-w-0">
                          <div className="truncate font-medium">{attachment.filename}</div>
                          <div className={`${dark ? "text-zinc-500" : "text-zinc-600"} text-xs`}>
                            {attachment.mime_type}
                          </div>
                        </div>
                        <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} shrink-0 text-xs`}>
                          {formatBytes(attachment.size_bytes)}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              <div className={`mt-6 whitespace-pre-wrap leading-7 ${dark ? "text-zinc-100" : "text-zinc-900"}`}>
                {detail.body}
              </div>
            </div>
          )}
        </div>
      </div>

      {contextMenu && (
        <div
          className={`fixed z-[60] w-44 rounded-xl border p-1 text-sm shadow-2xl ${
            dark ? "border-white/10 bg-[#151518] text-zinc-100" : "border-black/10 bg-white text-zinc-900"
          }`}
          style={{
            left: Math.min(contextMenu.x, window.innerWidth - 190),
            top: Math.min(contextMenu.y, window.innerHeight - 230),
          }}
          onClick={(event) => event.stopPropagation()}
        >
          {["重要", "待办", "工作"].map((tag) => (
            <button
              key={tag}
              onClick={() => void tagGroup(contextMenu.group, tag)}
              className={`w-full rounded-lg px-3 py-2 text-left transition ${dark ? "hover:bg-white/10" : "hover:bg-black/5"}`}
            >
              打标签：{tag}
            </button>
          ))}
          <button
            onClick={() => void clearTags(contextMenu.group)}
            className={`w-full rounded-lg px-3 py-2 text-left transition ${dark ? "hover:bg-white/10" : "hover:bg-black/5"}`}
          >
            清除标签
          </button>
          <div className={`my-1 h-px ${dark ? "bg-white/10" : "bg-black/10"}`} />
          <button
            onClick={() => void moveGroup(contextMenu.group, archiveFolder)}
            className={`w-full rounded-lg px-3 py-2 text-left transition ${dark ? "hover:bg-white/10" : "hover:bg-black/5"}`}
          >
            归档
          </button>
          <button
            onClick={() => void moveGroup(contextMenu.group, junkFolder)}
            className={`w-full rounded-lg px-3 py-2 text-left transition ${dark ? "hover:bg-white/10" : "hover:bg-black/5"}`}
          >
            标记垃圾
          </button>
        </div>
      )}

      {showCompose && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
          <div
            className={`w-full max-w-3xl rounded-3xl border p-6 ${
              dark ? "border-white/10 bg-[#0f0f12]" : "border-black/10 bg-white"
            }`}
          >
            <div className="flex items-center justify-between">
              <div className="text-lg font-semibold">写信</div>
              <button
                onClick={() => {
                  if (!sending) setShowCompose(false)
                }}
                className={`rounded-xl px-3 py-2 text-sm transition ${
                  dark ? "hover:bg-white/10" : "hover:bg-black/10"
                }`}
              >
                关闭
              </button>
            </div>

            <div className="mt-4 grid gap-3">
              <select
                value={fromAccountId}
                onChange={(event) => setFromAccountId(event.target.value)}
                className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
                  dark
                    ? "border-white/10 bg-[#151518] text-white"
                    : "border-black/10 bg-white text-black"
                }`}
              >
                {accounts.map((account) => (
                  <option key={account.id} value={account.id}>
                    发件账户：{account.email}
                  </option>
                ))}
              </select>
              <input
                value={toInput}
                onChange={(e) => setToInput(e.target.value)}
                placeholder="收件人（逗号或空格分隔）"
                className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
                  dark
                    ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                    : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
                }`}
              />
              <input
                value={subjectInput}
                onChange={(e) => setSubjectInput(e.target.value)}
                placeholder="主题"
                className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
                  dark
                    ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                    : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
                }`}
              />
              <textarea
                value={bodyInput}
                onChange={(e) => setBodyInput(e.target.value)}
                placeholder="正文"
                rows={10}
                className={`w-full resize-none rounded-2xl border px-4 py-3 text-sm outline-none ${
                  dark
                    ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                    : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
                }`}
              />
            </div>

            <div className="mt-5 flex justify-end gap-2">
              <button
                disabled={sending}
                onClick={() => setShowCompose(false)}
                className={`rounded-xl px-4 py-2 text-sm transition ${
                  dark ? "hover:bg-white/10" : "hover:bg-black/10"
                }`}
              >
                取消
              </button>
              <button
                disabled={!canSend}
                onClick={() => void onSend()}
                className={`rounded-xl px-4 py-2 text-sm font-medium transition ${
                  canSend
                    ? "bg-white text-black hover:scale-[1.02]"
                    : "bg-zinc-500/30 text-zinc-400"
                }`}
              >
                {sending ? "发送中..." : "发送"}
              </button>
            </div>
          </div>
        </div>
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

function mergeMessages(primary: MessageSummary[], secondary: MessageSummary[]) {
  const seen = new Set<string>()
  const merged: MessageSummary[] = []

  for (const message of [...primary, ...secondary]) {
    if (seen.has(message.id)) continue
    seen.add(message.id)
    merged.push(message)
  }

  return merged.sort((a, b) => b.date_ts - a.date_ts)
}

function getActiveAccountsForFolder(
  accounts: MailAccount[],
  accountFilterId: "all" | string,
  foldersByAccount: Record<string, MailFolderInfo[]>,
  folder: string,
) {
  const selectedAccounts =
    accountFilterId === "all"
      ? accounts
      : accounts.filter((account) => account.id === accountFilterId)
  const accountsWithFolder = selectedAccounts.filter((account) =>
    hasMatchingFolder(account.id, foldersByAccount, folder),
  )
  return accountsWithFolder.length > 0 ? accountsWithFolder : selectedAccounts
}

function resolveFolderForAccount(
  accountId: string,
  foldersByAccount: Record<string, MailFolderInfo[]>,
  selectedFolder: string,
) {
  const folders = foldersByAccount[accountId] ?? []
  const exact = folders.find((folder) => folder.selectable && folder.path === selectedFolder)
  if (exact) return exact.path

  const selectedRole = getFolderRole(selectedFolder, findFolderInfo(selectedFolder, foldersByAccount))
  if (selectedRole) {
    const roleMatch = folders.find((folder) =>
      folder.selectable && getFolderRole(folder.path, folder) === selectedRole
    )
    if (roleMatch) return roleMatch.path
  }

  return selectedFolder
}

function hasMatchingFolder(
  accountId: string,
  foldersByAccount: Record<string, MailFolderInfo[]>,
  selectedFolder: string,
) {
  const folders = foldersByAccount[accountId] ?? []
  if (folders.some((folder) => folder.selectable && folder.path === selectedFolder)) {
    return true
  }

  const selectedRole = getFolderRole(selectedFolder, findFolderInfo(selectedFolder, foldersByAccount))
  return Boolean(
    selectedRole &&
      folders.some((folder) => folder.selectable && getFolderRole(folder.path, folder) === selectedRole),
  )
}

function findFolderInfo(
  folder: string,
  foldersByAccount: Record<string, MailFolderInfo[]>,
) {
  for (const folders of Object.values(foldersByAccount)) {
    const match = folders.find((item) => item.path === folder)
    if (match) return match
  }
  return null
}

function findSentFolder(
  accountId: string,
  foldersByAccount: Record<string, MailFolderInfo[]>,
) {
  return (foldersByAccount[accountId] ?? []).find((folder) => isSentFolder(folder.path) && folder.selectable)?.path
}

function findActionFolder(
  foldersByAccount: Record<string, MailFolderInfo[]>,
  needles: string[],
) {
  for (const folders of Object.values(foldersByAccount)) {
    const match = folders.find((folder) => {
      if (!folder.selectable) return false
      const value = `${folder.path} ${folder.name}`.toLowerCase()
      return needles.some((needle) => value.includes(needle.toLowerCase()))
    })
    if (match) return match.path
  }
  return null
}

function isSentFolder(folder: string) {
  const value = folder.toLowerCase()
  return value.includes("sent") || value.includes("已发送") || value.includes("发件")
}

function getFolderRole(folder: string, info?: MailFolderInfo | null) {
  const value = `${folder} ${info?.name ?? ""}`.toLowerCase()

  if (value === "inbox" || value.includes("inbox") || value.includes("收件")) {
    return "inbox"
  }
  if (isSentFolder(value)) {
    return "sent"
  }
  if (value.includes("draft") || value.includes("草稿")) {
    return "drafts"
  }
  if (value.includes("spam") || value.includes("junk") || value.includes("垃圾")) {
    return "junk"
  }
  if (value.includes("trash") || value.includes("deleted") || value.includes("已删除")) {
    return "trash"
  }
  if (value.includes("archive") || value.includes("all mail") || value.includes("归档") || value.includes("所有邮件")) {
    return "archive"
  }

  return null
}

function groupMessagesByContact(messages: MessageSummary[], sentFolder: boolean): MessageGroup[] {
  const groups = new Map<string, MessageSummary[]>()

  for (const message of messages) {
    const contactKey = sentFolder
      ? message.to_emails.map((email) => email.trim().toLowerCase()).filter(Boolean).sort().join(",")
      : message.from_email.trim().toLowerCase() || message.from_name.trim().toLowerCase()
    const key = contactKey || message.id
    const current = groups.get(key)
    if (current) {
      current.push(message)
    } else {
      groups.set(key, [message])
    }
  }

  return Array.from(groups.entries())
    .map(([id, groupMessages]) => {
      const sorted = [...groupMessages].sort((a, b) => b.date_ts - a.date_ts)
      return {
        id,
        latest: sorted[0]!,
        messages: sorted,
        unreadCount: sentFolder ? 0 : sorted.filter((message) => !message.is_read).length,
        attachmentCount: sorted.reduce((sum, message) => sum + message.attachment_count, 0),
      }
    })
    .sort((a, b) => b.latest.date_ts - a.latest.date_ts)
}

export default MailScreen
