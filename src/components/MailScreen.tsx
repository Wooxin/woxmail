import { useEffect, useMemo, useRef, useState } from "react"
import { Paperclip, Plus, RefreshCcw } from "lucide-react"
import { useTranslation } from "react-i18next"

import type { MailAccount, MailFolder, MailFolderInfo, MessageDetail, MessageSummary } from "../types/mail"
import {
  addMessageTag,
  clearMessageTags,
  deleteComposeDraft,
  getComposeDraft,
  getMessage,
  listMessages,
  markMessageRead,
  moveMessagesToFolder,
  openAttachment,
  saveAttachment,
  saveComposeDraft,
  searchMessages,
  sendMessage,
  syncFolder,
  syncFolderDeep,
  translateMessage,
  getThread,
} from "../api/mail"
import HtmlRenderer from "./HtmlRenderer"
import { listContacts } from "../api/contact"
import { openExternalUrl } from "../api/open"
import { folderDisplayName } from "../utils/folders"

function linkifyUrls(text: string): string {
  if (!text) return text
  // Match http/https URLs
  return text.replace(
    /(https?:\/\/[^\s<>"']+)/g,
    '<a href="$1" class="auto-link" target="_blank" rel="noopener noreferrer">$1</a>'
  )
}

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
const composeDraftScope = "compose:new"
const signatureStorageKey = "woxmail.composeSignature"

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

function looksLikeHtml(text: string): boolean {
  if (!text || text.length < 10) return false
  const lower = text.slice(0, 2000).toLowerCase()
  return /<\s*(html|body|div|table|p|br|a\s|img\s|span|style|head|meta)/.test(lower)
}

function parseRecipientInput(value: string) {
  return value
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean)
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
  const [translating, setTranslating] = useState(false)
  const [translatedBody, setTranslatedBody] = useState<string | null>(null)
  const [threadMessages, setThreadMessages] = useState<MessageSummary[] | null>(null)
  const [threadLoading, setThreadLoading] = useState(false)
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [messages, setMessages] = useState<MessageSummary[]>([])
  const [searchInput, setSearchInput] = useState("")
  const [searching, setSearching] = useState(false)
  const [hasMore, setHasMore] = useState(false)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<MessageDetail | null>(null)
  const [showCompose, setShowCompose] = useState(false)
  const [composeMode, setComposeMode] = useState<"new" | "reply" | "forward">("new")
  const [contextMenu, setContextMenu] = useState<{
    x: number
    y: number
    group: MessageGroup
  } | null>(null)
  const lastSyncAtRef = useRef<Record<string, number>>({})

  const [fromAccountId, setFromAccountId] = useState(accounts[0]?.id ?? "")
  const [toInput, setToInput] = useState("")
  const [contactSuggestions, setContactSuggestions] = useState<{ name: string; email: string }[]>([])
  const [suggestionIndex, setSuggestionIndex] = useState(-1)
  const [subjectInput, setSubjectInput] = useState("")
  const [bodyInput, setBodyInput] = useState("")
  const [signatureInput, setSignatureInput] = useState(() =>
    window.localStorage.getItem(signatureStorageKey) ?? "",
  )
  const [draftStatus, setDraftStatus] = useState<"idle" | "saving" | "saved">("idle")
  const bodyEditorRef = useRef<HTMLDivElement | null>(null)

  const selectedSummary = useMemo(
    () => messages.find((m) => m.id === selectedId) ?? null,
    [messages, selectedId],
  )

  const searchQuery = searchInput.trim()
  const isSearching = searchQuery.length > 0
  const accountsById = useMemo(
    () => new Map(accounts.map((account) => [account.id, account])),
    [accounts],
  )

  const messageGroups = useMemo(
    () => groupMessagesByContact(messages, isSearching ? false : isSentFolder(folder)),
    [folder, isSearching, messages],
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

  useEffect(() => {
    if (!showCompose || !bodyEditorRef.current) return
    if (bodyEditorRef.current.innerHTML !== bodyInput) {
      bodyEditorRef.current.innerHTML = bodyInput
    }
  }, [bodyInput, showCompose])

  useEffect(() => {
    if (!showCompose || composeMode !== "new") return

    let cancelled = false
    void (async () => {
      try {
        const draft = await getComposeDraft(composeDraftScope)
        if (cancelled || !draft) return
        if (accounts.some((account) => account.id === draft.account_id)) {
          setFromAccountId(draft.account_id)
        }
        setToInput(draft.to_emails.join(", "))
        setSubjectInput(draft.subject)
        setBodyInput(draft.body)
        setDraftStatus("saved")
      } catch {
        // Draft restore is a convenience; composing should still work if it fails.
      }
    })()

    return () => {
      cancelled = true
    }
  }, [accounts, composeMode, showCompose])

  useEffect(() => {
    if (!showCompose || sending || composeMode !== "new") return
    if (!fromAccountId && !toInput.trim() && !subjectInput.trim() && !bodyInput.trim()) return

    let cancelled = false
    const timer = window.setTimeout(() => {
      void (async () => {
        setDraftStatus("saving")
        try {
          await saveComposeDraft({
            scope: composeDraftScope,
            accountId: fromAccountId || (accounts[0]?.id ?? ""),
            toEmails: parseRecipientInput(toInput),
            subject: subjectInput,
            body: bodyInput,
          })
          if (!cancelled) setDraftStatus("saved")
        } catch {
          if (!cancelled) setDraftStatus("idle")
        }
      })()
    }, 800)

    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [accounts, bodyInput, composeMode, fromAccountId, sending, showCompose, subjectInput, toInput])

  useEffect(() => {
    if (!isSearching) return

    let cancelled = false
    const timer = window.setTimeout(() => {
      void (async () => {
        setSearching(true)
        setError(null)
        try {
          const results = await searchMessages({
            query: searchQuery,
            accountId: accountFilterId,
            limit: 150,
          })
          if (cancelled) return
          setMessages(results)
          setHasMore(false)
          setSelectedId((current) =>
            current && results.some((message) => message.id === current)
              ? current
              : results[0]?.id ?? null,
          )
        } catch (searchError) {
          if (!cancelled) setError(getErrorMessage(searchError))
        } finally {
          if (!cancelled) setSearching(false)
        }
      })()
    }, 250)

    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [accountFilterId, isSearching, searchQuery])

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
    if (isSearching && !options?.skipSync) return

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

      // Fire sync in background — don't block UI
      if (!options?.skipSync && !append) {
        const now = Date.now()
        const accountsToSync = targetAccounts.filter((account) => {
          const accountFolder = accountFolders.get(account.id) ?? nextFolder
          const key = `${account.id}:${accountFolder}`
          return options?.forceSync || now - (lastSyncAtRef.current[key] ?? 0) > syncCooldownMs
        })

        if (accountsToSync.length > 0) {
          // Don't await — run sync in background
          Promise.all(accountsToSync.map(async (account) => {
            const accountFolder = accountFolders.get(account.id) ?? nextFolder
            await syncFolder(account.id, accountFolder)
            lastSyncAtRef.current[`${account.id}:${accountFolder}`] = Date.now()
          })).then(async () => {
            await onUnreadCountsChanged()
            applyMessages(await readMessages())
          }).catch(() => {})
        }
      }

      if (append || background) {
        applyMessages(await readMessages())
      }
    } catch (loadError) {
      if (!background) setError(getErrorMessage(loadError))
    } finally {
      setLoading(false)
      setSyncing(false)
      setLoadingMore(false)
    }
  }

  useEffect(() => {
    if (isSearching) return
    // Don't clear messages — keep old list until new data arrives
    setHasMore(false)
    void loadMessages(folder)
  }, [accountFilterId, accounts.length, isSearching])

  useEffect(() => {
    if (isSearching) return
    // Don't clear messages — keep old list until new data arrives
    setHasMore(false)
    void loadMessages(folder)
  }, [folder, isSearching])

  useEffect(() => {
    if (isSearching) return
    const id = window.setInterval(() => {
      if (loading || syncing || loadingMore || sending) return
      void loadMessages(folder, selectedId ?? undefined, { background: true })
    }, backgroundSyncMs)

    return () => window.clearInterval(id)
  }, [accountFilterId, accounts.length, folder, isSearching, loading, syncing, loadingMore, sending, selectedId, messages.length])

  useEffect(() => {
    if (!selectedId) {
      setDetail(null)
      return
    }
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | null = null
    void (async () => {
      setDetailLoading(true)
      setError(null)
      try {
        const msg = await getMessage(selectedId)
        if (cancelled) return
        // Debounce: only show detail after a short delay to avoid flicker on rapid clicks
        timer = setTimeout(() => {
          if (cancelled) return
          setDetail(msg)
          setDetailLoading(false)
        }, 50)
      } catch (detailError) {
        if (!cancelled) {
          setError(getErrorMessage(detailError))
          setDetailLoading(false)
        }
      }

      // Mark as read (don't block detail display)
      if (!cancelled) {
        await markMessageRead(selectedId).catch(() => {})
        if (cancelled) return
        setDetail((prev) => prev ? { ...prev, is_read: true } : null)
        setMessages((current) =>
          current.map((message) =>
            message.id === selectedId ? { ...message, is_read: true } : message,
          ),
        )
        void onUnreadCountsChanged()
      }
    })()
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [selectedId])

  const onSend = async () => {
    const to = parseRecipientInput(toInput)
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
        is_html: true,
      })

      setShowCompose(false)
      setToInput("")
      setSubjectInput("")
      setBodyInput("")
      setDraftStatus("idle")
      await deleteComposeDraft(composeDraftScope).catch(() => undefined)
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

  const deepSyncCurrentFolder = async () => {
    if (isSearching || loading || syncing || loadingMore) return

    setSyncing(true)
    setError(null)
    try {
      const targetAccounts = getActiveAccountsForFolder(
        accounts,
        accountFilterId,
        foldersByAccount,
        folder,
      )
      const accountFolders = new Map(
        targetAccounts.map((account) => [
          account.id,
          resolveFolderForAccount(account.id, foldersByAccount, folder),
        ]),
      )
      let didSync = false
      await Promise.all(targetAccounts.map(async (account) => {
        const accountFolder = accountFolders.get(account.id) ?? folder
        await syncFolderDeep(account.id, accountFolder)
        didSync = true
        lastSyncAtRef.current[`${account.id}:${accountFolder}`] = Date.now()
      }))
      if (didSync) {
        void onUnreadCountsChanged()
      }
      await loadMessages(folder, selectedId ?? undefined, { skipSync: true })
    } catch (syncError) {
      setError(getErrorMessage(syncError))
    } finally {
      setSyncing(false)
    }
  }

  const closeContextMenu = () => setContextMenu(null)

  const openNewCompose = () => {
    setComposeMode("new")
    if (!toInput.trim() && !subjectInput.trim() && !bodyInput.trim() && signatureInput.trim()) {
      setBodyInput(`<br><br>${signatureHtml(signatureInput)}`)
    }
    setShowCompose(true)
  }

  const replyToDetail = () => {
    if (!detail) return
    setComposeMode("reply")
    setToInput(detail.from_email)
    setSubjectInput(replySubject(detail.subject))
    setBodyInput(`<br><br>${quotedMessageHtml(detail)}`)
    setShowCompose(true)
    setDraftStatus("idle")
  }

  const forwardDetail = () => {
    if (!detail) return
    setComposeMode("forward")
    setToInput("")
    setSubjectInput(forwardSubject(detail.subject))
    setBodyInput(`<br><br>${quotedMessageHtml(detail)}`)
    setShowCompose(true)
    setDraftStatus("idle")
  }

  const runFormatCommand = (command: string, value?: string) => {
    bodyEditorRef.current?.focus()
    document.execCommand(command, false, value)
    setBodyInput(bodyEditorRef.current?.innerHTML ?? "")
  }

  const updateSignature = (value: string) => {
    setSignatureInput(value)
    window.localStorage.setItem(signatureStorageKey, value)
  }

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

  const saveDetailAttachment = async (attachmentId: string) => {
    setError(null)
    try {
      await saveAttachment(attachmentId)
    } catch (attachmentError) {
      setError(getErrorMessage(attachmentError))
    }
  }

  const openDetailAttachment = async (attachmentId: string) => {
    setError(null)
    try {
      await openAttachment(attachmentId)
    } catch (attachmentError) {
      setError(getErrorMessage(attachmentError))
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
            {searching ? "正在搜索..." : isSearching ? `搜索：${searchQuery}` : loading ? "正在加载..." : syncing ? "后台同步中..." : accountFilterId === "all" ? "全部邮箱" : activeAccounts[0]?.email}
          </div>
        </div>

        <div className="flex items-center gap-2">
          <input
            value={searchInput}
            onChange={(event) => setSearchInput(event.target.value)}
            placeholder="搜索邮件"
            className={`w-56 rounded-xl border px-3 py-2 text-sm outline-none ${
              dark
                ? "border-white/10 bg-[#151518] text-white placeholder:text-zinc-500"
                : "border-black/10 bg-white text-black placeholder:text-zinc-500"
            }`}
          />
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
            disabled={loading || isSearching}
            onClick={() => void loadMessages(folder, selectedId ?? undefined, { forceSync: true })}
            className={`flex items-center gap-2 rounded-xl px-3 py-2 text-sm transition ${
              dark ? "hover:bg-white/10" : "hover:bg-black/10"
            }`}
          >
            <RefreshCcw size={16} />
            {loading ? "刷新中" : "刷新"}
          </button>
          <button
            disabled={loading || syncing || isSearching}
            onClick={() => void deepSyncCurrentFolder()}
            className={`rounded-xl px-3 py-2 text-sm transition ${
              dark ? "hover:bg-white/10" : "hover:bg-black/10"
            } disabled:opacity-50`}
          >
            {syncing ? "同步中" : "同步更多"}
          </button>
          <button
            onClick={openNewCompose}
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
          {(loading || searching) && messages.length === 0 ? (
            <div className={`p-6 ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              {searching ? "正在搜索邮件..." : "正在加载邮件..."}
            </div>
          ) : messages.length === 0 ? (
            <div className={`p-6 ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              {isSearching ? "没有找到匹配邮件" : "暂无邮件"}
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
                    {isSearching
                      ? `${accountsById.get(m.account_id)?.email ?? "邮箱"} · ${folderDisplayName(findFolderInfo(m.folder_path, foldersByAccount) ?? m.folder_path, t)}`
                      : isSentFolder(folder)
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
              <div className="mt-4 flex gap-2">
                <button
                  onClick={replyToDetail}
                  className={`rounded-xl px-3 py-2 text-sm transition ${
                    dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                  }`}
                >
                  回复
                </button>
                <button
                  onClick={forwardDetail}
                  className={`rounded-xl px-3 py-2 text-sm transition ${
                    dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                  }`}
                >
                  转发
                </button>
                <button
                  onClick={() => {
                    if (!detail) return
                    if (translatedBody) { setTranslatedBody(null); return }
                    setTranslating(true)
                    translateMessage(detail.body)
                      .then(setTranslatedBody)
                      .catch((e) => setError(getErrorMessage(e)))
                      .finally(() => setTranslating(false))
                  }}
                  className={`rounded-xl px-3 py-2 text-sm transition ${
                    dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                  }`}
                >
                  {translating ? "翻译中..." : translatedBody ? "显示原文" : "翻译"}
                </button>
                <button
                  onClick={() => {
                    if (!detail) return
                    if (threadMessages) { setThreadMessages(null); return }
                    setThreadLoading(true)
                    getThread(detail.id, detail.account_id)
                      .then(setThreadMessages)
                      .catch(() => {})
                      .finally(() => setThreadLoading(false))
                  }}
                  className={`rounded-xl px-3 py-2 text-sm transition ${
                    dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                  }`}
                >
                  {threadLoading ? "加载中..." : threadMessages ? "收起会话" : "会话"}
                </button>
              </div>
              <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} mt-3 text-sm`}>
                From: {detail.from_name} &lt;{detail.from_email}&gt;
              </div>
              <div className={`${dark ? "text-zinc-400" : "text-zinc-600"} mt-1 text-sm`}>
                To: {detail.to_emails.join(", ")}
              </div>
              <div className={`${dark ? "text-zinc-500" : "text-zinc-600"} mt-1 text-sm`}>
                Date: {formatTs(detail.date_ts)}
              </div>

              {threadMessages && threadMessages.length > 1 && (
                <div className={`mt-4 rounded-2xl border p-3 ${dark ? "border-white/10 bg-white/[0.03]" : "border-black/10 bg-black/[0.03]"}`}>
                  <div className="mb-2 text-sm font-semibold">
                    会话 ({threadMessages.length} 封)
                  </div>
                  <div className="space-y-1 max-h-64 overflow-y-auto">
                    {threadMessages.map((tm) => (
                      <button
                        key={tm.id}
                        onClick={() => setSelectedId(tm.id)}
                        className={`w-full rounded-xl px-3 py-2 text-left text-sm transition ${
                          tm.id === detail.id
                            ? dark ? "bg-blue-500/20" : "bg-blue-500/10"
                            : dark ? "hover:bg-white/5" : "hover:bg-black/5"
                        }`}
                      >
                        <div className="flex items-center gap-2">
                          {!tm.is_read && <span className="h-2 w-2 shrink-0 rounded-full bg-blue-500" />}
                          <span className="font-medium truncate">{tm.from_name}</span>
                          <span className={`truncate text-xs ${dark ? "text-zinc-500" : "text-zinc-400"}`}>
                            {formatTs(tm.date_ts)}
                          </span>
                        </div>
                        <div className="truncate text-xs mt-0.5">{tm.subject}</div>
                      </button>
                    ))}
                  </div>
                </div>
              )}

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
                        <div className="flex shrink-0 items-center gap-2">
                          <span className={`${dark ? "text-zinc-400" : "text-zinc-600"} text-xs`}>
                            {formatBytes(attachment.size_bytes)}
                          </span>
                          <button
                            onClick={() => void openDetailAttachment(attachment.id)}
                            className={`rounded-lg px-2 py-1 text-xs transition ${
                              dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                            }`}
                          >
                            打开
                          </button>
                          <button
                            onClick={() => void saveDetailAttachment(attachment.id)}
                            className={`rounded-lg px-2 py-1 text-xs transition ${
                              dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                            }`}
                          >
                            保存
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              <div className={`mt-6 whitespace-pre-wrap leading-7 ${dark ? "text-zinc-100" : "text-zinc-900"}`}>
                {translatedBody ? (
                  <div>
                    <div className={`mb-3 rounded-xl px-4 py-2 text-sm ${dark ? "bg-blue-500/10 text-blue-300" : "bg-blue-500/5 text-blue-600"}`}>
                      🌐 翻译结果 (Bing Translator)
                    </div>
                    {translatedBody}
                  </div>
                ) : detail.body_html ? (
                  <HtmlRenderer html={detail.body_html} text={detail.body} dark={dark} />
                ) : looksLikeHtml(detail.body) ? (
                  <HtmlRenderer html={detail.body} text={detail.body} dark={dark} />
                ) : (
                  <div
                    ref={(el) => {
                      if (!el) return
                      const handler = (e: MouseEvent) => {
                        const anchor = (e.target as HTMLElement).closest("a")
                        if (anchor?.href?.startsWith("http")) {
                          e.preventDefault()
                          openExternalUrl(anchor.href)
                        }
                      }
                      el.addEventListener("click", handler)
                    }}
                    dangerouslySetInnerHTML={{ __html: linkifyUrls(detail.body).replace(/\n/g, "<br>") }}
                  />
                )}
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
          <div className={`my-1 h-px ${dark ? "bg-white/10" : "bg-black/10"}`} />
          <button
            onClick={() => {
              closeContextMenu()
              void (async () => {
                const ids = contextMenu.group.messages.map((m) => m.id)
                for (const id of ids) {
                  await markMessageRead(id).catch(() => {})
                }
                setMessages((current) =>
                  current.map((m) => (ids.includes(m.id) ? { ...m, is_read: true } : m)),
                )
                void onUnreadCountsChanged()
              })()
            }}
            className={`w-full rounded-lg px-3 py-2 text-left transition ${dark ? "hover:bg-white/10" : "hover:bg-black/5"}`}
          >
            标记已读
          </button>
        </div>
      )}

      {showCompose && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
          onClick={(e) => {
            if (e.target === e.currentTarget && !sending) setShowCompose(false)
          }}
        >
          <div
            className={`w-full max-w-3xl max-h-[90vh] overflow-y-auto rounded-3xl border p-6 ${
              dark ? "border-white/10 bg-[#0f0f12]" : "border-black/10 bg-white"
            }`}
          >
            <div className="flex items-center justify-between">
              <div>
                <div className="text-lg font-semibold">
                  {composeMode === "reply" ? "回复" : composeMode === "forward" ? "转发" : "写信"}
                </div>
                <div className={`${dark ? "text-zinc-500" : "text-zinc-600"} mt-1 text-xs`}>
                  {draftStatus === "saving" ? "正在保存草稿..." : draftStatus === "saved" ? "草稿已保存" : "本地草稿"}
                </div>
              </div>
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
              <div className="relative">
                <input
                  value={toInput}
                  onChange={async (e) => {
                    setToInput(e.target.value)
                    setSuggestionIndex(-1)
                    const val = e.target.value
                    const lastPart = val.split(/[,\s]+/).pop() ?? ""
                    if (lastPart.length >= 1) {
                      try {
                        const contacts = await listContacts(lastPart)
                        setContactSuggestions(contacts.slice(0, 5))
                      } catch { setContactSuggestions([]) }
                    } else { setContactSuggestions([]) }
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "ArrowDown") {
                      e.preventDefault()
                      setSuggestionIndex((i) => Math.min(i + 1, contactSuggestions.length - 1))
                    } else if (e.key === "ArrowUp") {
                      e.preventDefault()
                      setSuggestionIndex((i) => Math.max(i - 1, -1))
                    } else if (e.key === "Enter" && suggestionIndex >= 0) {
                      e.preventDefault()
                      const sel = contactSuggestions[suggestionIndex]
                      if (sel) {
                        const parts = toInput.split(/[,\s]+/).filter(Boolean)
                        parts.pop()
                        parts.push(`${sel.name} <${sel.email}>`)
                        setToInput(parts.join(", ") + ", ")
                        setContactSuggestions([])
                        setSuggestionIndex(-1)
                      }
                    } else if (e.key === "Escape") {
                      setContactSuggestions([])
                      setSuggestionIndex(-1)
                    }
                  }}
                  placeholder="收件人（逗号或空格分隔）"
                  className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
                    dark
                      ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                      : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
                  }`}
                />
                {contactSuggestions.length > 0 && (
                  <div className={`absolute left-0 right-0 top-full z-50 mt-1 rounded-2xl border p-1 shadow-2xl ${
                    dark ? "border-white/10 bg-[#151518]" : "border-black/10 bg-white"
                  }`}>
                    {contactSuggestions.map((c, idx) => (
                      <button
                        key={c.email}
                        onClick={() => {
                          const parts = toInput.split(/[,\s]+/).filter(Boolean)
                          parts.pop()
                          parts.push(`${c.name} <${c.email}>`)
                          setToInput(parts.join(", ") + ", ")
                          setContactSuggestions([])
                          setSuggestionIndex(-1)
                        }}
                        className={`flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-sm transition ${
                          idx === suggestionIndex
                            ? dark ? "bg-white/10" : "bg-black/10"
                            : dark ? "hover:bg-white/5" : "hover:bg-black/5"
                        }`}
                      >
                        <span className="flex-1 truncate">{c.name}</span>
                        <span className={dark ? "text-zinc-500" : "text-zinc-400"}>{c.email}</span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
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
              <div
                className={`flex flex-wrap items-center gap-2 rounded-2xl border px-3 py-2 ${
                  dark
                    ? "border-white/10 bg-white/5"
                    : "border-black/10 bg-black/5"
                }`}
              >
                {[
                  ["bold", "B"],
                  ["italic", "I"],
                  ["underline", "U"],
                ].map(([command, label]) => (
                  <button
                    key={command}
                    type="button"
                    onClick={() => runFormatCommand(command)}
                    className={`h-8 min-w-8 rounded-lg px-2 text-sm font-semibold transition ${
                      dark ? "hover:bg-white/10" : "hover:bg-black/10"
                    }`}
                  >
                    {label}
                  </button>
                ))}
                <button
                  type="button"
                  onClick={() => runFormatCommand("insertUnorderedList")}
                  className={`h-8 rounded-lg px-2 text-sm transition ${
                    dark ? "hover:bg-white/10" : "hover:bg-black/10"
                  }`}
                >
                  列表
                </button>
                <select
                  onChange={(event) => {
                    if (event.target.value) runFormatCommand("formatBlock", event.target.value)
                    event.target.value = ""
                  }}
                  className={`h-8 rounded-lg border px-2 text-sm outline-none ${
                    dark ? "border-white/10 bg-[#151518] text-white" : "border-black/10 bg-white text-black"
                  }`}
                >
                  <option value="">样式</option>
                  <option value="p">正文</option>
                  <option value="h2">标题</option>
                  <option value="blockquote">引用</option>
                </select>
              </div>
              <div
                ref={bodyEditorRef}
                contentEditable
                suppressContentEditableWarning
                onInput={(event) => setBodyInput(event.currentTarget.innerHTML)}
                data-placeholder="正文"
                className={`min-h-64 w-full overflow-y-auto rounded-2xl border px-4 py-3 text-sm leading-7 outline-none ${
                  dark
                    ? "border-white/10 bg-white/5 text-white"
                    : "border-black/10 bg-black/5 text-black"
                }`}
              />
              <textarea
                value={signatureInput}
                onChange={(event) => updateSignature(event.target.value)}
                placeholder="签名（新邮件会自动带上）"
                rows={3}
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

function replySubject(subject: string) {
  return subject.trim().toLowerCase().startsWith("re:")
    ? subject
    : `Re: ${subject}`
}

function forwardSubject(subject: string) {
  return subject.trim().toLowerCase().startsWith("fw:")
    || subject.trim().toLowerCase().startsWith("fwd:")
    ? subject
    : `Fwd: ${subject}`
}

function signatureHtml(value: string) {
  return `<div>--</div><div>${escapeHtml(value).replace(/\n/g, "<br>")}</div>`
}

function quotedMessageHtml(message: MessageDetail) {
  const header = [
    "---- 原始邮件 ----",
    `From: ${message.from_name} <${message.from_email}>`,
    `To: ${message.to_emails.join(", ")}`,
    `Date: ${formatTs(message.date_ts)}`,
    `Subject: ${message.subject}`,
    "",
  ].join("\n")
  return `<blockquote style="border-left:3px solid #94a3b8;margin:0;padding-left:12px;color:#64748b">${escapeHtml(`${header}${message.body}`).replace(/\n/g, "<br>")}</blockquote>`
}

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;")
}

export default MailScreen
