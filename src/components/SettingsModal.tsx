import { useEffect, useState } from "react"
import { X, Database, Trash2, RefreshCw, Loader2 } from "lucide-react"
import type { CacheSettings, CacheStats } from "../types/mail"
import { getCacheSettings, setCacheSettings, getCacheStats, clearCache, purgeOldMessages } from "../api/cache"
import { importMbox, importEml } from "../api/mail"
import { listFilterRules, createFilterRule, deleteFilterRule, toggleFilterRule, type FilterRule } from "../api/mail"
import type { MailAccount, MailFolderInfo } from "../types/mail"
import { listAccounts, listFolders } from "../api/mail"

type SidebarAccountMode = "list" | "dropdown"
type TabId = "general" | "cache" | "import" | "rules" | "translate"

type Props = {
  dark: boolean
  sidebarAccountMode: SidebarAccountMode
  onSidebarAccountModeChange: (mode: SidebarAccountMode) => void
  onClose: () => void
}

function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B"
  const units = ["B", "KB", "MB", "GB"]
  let size = bytes, idx = 0
  while (size >= 1024 && idx < units.length - 1) { size /= 1024; idx++ }
  return `${size >= 10 ? size.toFixed(0) : size.toFixed(1)} ${units[idx]}`
}

function fieldLabel(f: string) { return f === "from" ? "发件人" : f === "subject" ? "主题" : f === "to" ? "收件人" : f }
function opLabel(o: string) { return o === "contains" ? "包含" : o === "equals" ? "等于" : o === "starts_with" ? "开头是" : o }
function actionLabel(t: string, v: string) { return t === "tag" ? `打标签"${v}"` : `移动到${v}` }

function SettingsModal({
  dark,
  sidebarAccountMode,
  onSidebarAccountModeChange,
  onClose,
}: Props) {
  const [activeTab, setActiveTab] = useState<TabId>("general")
  const [cacheSettings, setCacheSet] = useState<CacheSettings | null>(null)
  const [cacheStats, setCacheStats] = useState<CacheStats | null>(null)
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [actionMsg, setActionMsg] = useState<string | null>(null)

  // Import state
  const [importAccounts, setImportAccounts] = useState<MailAccount[]>([])
  const [importFolders, setImportFolders] = useState<MailFolderInfo[]>([])
  const [importAccountId, setImportAccountId] = useState("")
  const [importFolderPath, setImportFolderPath] = useState("INBOX")
  const [importFilePath, setImportFilePath] = useState("")
  const [importing, setImporting] = useState(false)
  const [importMsg, setImportMsg] = useState<string | null>(null)

  // Rules state
  const [rules, setRules] = useState<FilterRule[]>([])
  const [ruleForm, setRuleForm] = useState({ name: "", field: "from", operator: "contains", value: "", action_type: "tag", action_value: "" })
  const [ruleSaving, setRuleSaving] = useState(false)
  const [ruleMsg, setRuleMsg] = useState<string | null>(null)

  // Translate settings state
  const [translateAppid, setTranslateAppid] = useState(() => window.localStorage.getItem("woxmail.translateAppid") ?? "")
  const [translateSecret, setTranslateSecret] = useState(() => window.localStorage.getItem("woxmail.translateSecret") ?? "")
  const [translateSaved, setTranslateSaved] = useState(false)

  const loadRules = async () => {
    try { setRules(await listFilterRules()) } catch { /* ignore */ }
  }

  useEffect(() => { if (activeTab === "rules") void loadRules() }, [activeTab])

  const handleCreateRule = async () => {
    if (!ruleForm.name.trim() || !ruleForm.value.trim()) return
    setRuleSaving(true)
    setRuleMsg(null)
    try {
      await createFilterRule(ruleForm)
      setRuleForm({ name: "", field: "from", operator: "contains", value: "", action_type: "tag", action_value: "" })
      await loadRules()
      setRuleMsg("规则已创建")
    } catch (e) {
      setRuleMsg(`失败: ${e instanceof Error ? e.message : String(e)}`)
    } finally { setRuleSaving(false) }
  }

  const handleDeleteRule = async (id: string) => {
    try { await deleteFilterRule(id); await loadRules() } catch { /* ignore */ }
  }

  const handleToggleRule = async (id: string, enabled: boolean) => {
    try { await toggleFilterRule(id, enabled); await loadRules() } catch { /* ignore */ }
  }

  const loadCacheData = async () => {
    setLoading(true)
    try {
      const [settings, stats] = await Promise.all([getCacheSettings(), getCacheStats()])
      setCacheSet(settings)
      setCacheStats(stats)
    } catch {
      // Silent fail
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (activeTab === "cache") void loadCacheData()
  }, [activeTab])

  useEffect(() => {
    if (activeTab !== "import") return
    void (async () => {
      try {
        const accts = await listAccounts()
        setImportAccounts(accts)
        if (accts.length > 0 && !importAccountId) {
          setImportAccountId(accts[0].id)
          const folders = await listFolders(accts[0].id)
          setImportFolders(folders)
          const inbox = folders.find((f) => f.path.toLowerCase() === "inbox")
          setImportFolderPath(inbox?.path ?? "INBOX")
        }
      } catch { /* ignore */ }
    })()
  }, [activeTab])

  const handleAccountChange = async (accountId: string) => {
    setImportAccountId(accountId)
    try {
      const folders = await listFolders(accountId)
      setImportFolders(folders)
      const inbox = folders.find((f) => f.path.toLowerCase() === "inbox")
      setImportFolderPath(inbox?.path ?? "INBOX")
    } catch { setImportFolders([]) }
  }

  const handleImport = async () => {
    if (!importAccountId || !importFilePath.trim()) return
    setImporting(true)
    setImportMsg(null)
    try {
      const isEml = importFilePath.toLowerCase().endsWith(".eml")
      const count = isEml
        ? await importEml(importAccountId, importFolderPath, importFilePath.trim())
        : await importMbox(importAccountId, importFolderPath, importFilePath.trim())
      setImportMsg(`成功导入 ${count} 封邮件到 ${importFolderPath}`)
      setImportFilePath("")
    } catch (e) {
      setImportMsg(`失败: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setImporting(false)
    }
  }

  const handleSaveCache = async () => {
    if (!cacheSettings) return
    setSaving(true)
    setActionMsg(null)
    try {
      await setCacheSettings(cacheSettings)
      setActionMsg("已保存")
      setTimeout(() => setActionMsg(null), 2000)
    } catch (e) {
      setActionMsg(`保存失败: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSaving(false)
    }
  }

  const handleClearCache = async () => {
    if (!window.confirm("确定要清除所有附件缓存？附件需要再次手动下载。邮件正文不受影响。")) return
    setSaving(true)
    setActionMsg(null)
    try {
      const count = await clearCache()
      setActionMsg(`已清除 ${count} 个附件缓存`)
      await loadCacheData()
    } catch (e) {
      setActionMsg(`清除失败: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSaving(false)
    }
  }

  const handlePurge = async () => {
    if (!cacheSettings) return
    if (!window.confirm(`确定要删除 ${cacheSettings.body_retention_days} 天前的旧邮件正文和附件？此操作不可恢复。`)) return
    setSaving(true)
    setActionMsg(null)
    try {
      const count = await purgeOldMessages(cacheSettings.body_retention_days)
      setActionMsg(`已清理 ${count} 封旧邮件`)
      await loadCacheData()
    } catch (e) {
      setActionMsg(`清理失败: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSaving(false)
    }
  }

  const tabs: { id: TabId; label: string }[] = [
    { id: "general", label: "常规" },
    { id: "cache", label: "缓存" },
    { id: "import", label: "导入" },
    { id: "rules", label: "规则" },
    { id: "translate", label: "翻译" },
  ]

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md">
      <div
        className={`w-[560px] max-h-[85vh] overflow-y-auto rounded-3xl border p-6 shadow-2xl ${
          dark ? "border-white/10 bg-[#111]" : "border-black/10 bg-white"
        }`}
      >
        <div className="flex items-center justify-between">
          <h2 className="text-2xl font-bold">设置</h2>
          <button
            onClick={onClose}
            className={`rounded-xl p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}
          >
            <X size={20} />
          </button>
        </div>

        {/* Tabs */}
        <div className={`mt-4 flex gap-1 rounded-xl p-1 ${dark ? "bg-white/5" : "bg-black/5"}`}>
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex-1 rounded-lg px-4 py-2 text-sm font-medium transition ${
                activeTab === tab.id
                  ? dark ? "bg-white/10 text-white" : "bg-white text-black shadow-sm"
                  : dark ? "text-zinc-400 hover:text-white" : "text-zinc-600 hover:text-black"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* General Tab */}
        {activeTab === "general" && (
          <div className="mt-5">
            <div className="text-sm font-semibold">左侧邮箱显示</div>
            <div className="mt-3 grid gap-2">
              <label
                className={`flex cursor-pointer items-start gap-3 rounded-2xl border p-4 ${
                  dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"
                }`}
              >
                <input
                  checked={sidebarAccountMode === "list"}
                  onChange={() => onSidebarAccountModeChange("list")}
                  type="radio" name="sidebarAccountMode" className="mt-1"
                />
                <span>
                  <span className="block font-medium">显示全部邮箱账号</span>
                  <span className={`mt-1 block text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                    左侧直接列出全部账号，适合账号少、频繁切换的使用方式。
                  </span>
                </span>
              </label>
              <label
                className={`flex cursor-pointer items-start gap-3 rounded-2xl border p-4 ${
                  dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"
                }`}
              >
                <input
                  checked={sidebarAccountMode === "dropdown"}
                  onChange={() => onSidebarAccountModeChange("dropdown")}
                  type="radio" name="sidebarAccountMode" className="mt-1"
                />
                <span>
                  <span className="block font-medium">使用下拉菜单选择邮箱</span>
                  <span className={`mt-1 block text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                    左侧更紧凑，默认查看全部邮件，需要时从下拉菜单筛选具体邮箱。
                  </span>
                </span>
              </label>
            </div>
          </div>
        )}

        {/* Cache Tab */}
        {activeTab === "cache" && (
          <div className="mt-5 space-y-5">
            {/* Stats */}
            {cacheStats && (
              <div className={`rounded-2xl border p-4 ${dark ? "border-white/10 bg-white/[0.03]" : "border-black/10 bg-black/[0.03]"}`}>
                <div className="flex items-center gap-2 text-sm font-semibold">
                  <Database size={16} />
                  本地缓存统计
                </div>
                <div className={`mt-3 grid grid-cols-2 gap-3 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                  <div>缓存邮件: <span className="font-medium text-inherit">{cacheStats.message_count} 封</span></div>
                  <div>附件数量: <span className="font-medium text-inherit">{cacheStats.attachment_count} 个</span></div>
                  <div>正文大小: <span className="font-medium text-inherit">{formatBytes(cacheStats.total_body_bytes)}</span></div>
                  <div>附件大小: <span className="font-medium text-inherit">{formatBytes(cacheStats.total_attachment_bytes)}</span></div>
                  <div className="col-span-2">数据库文件: <span className="font-medium text-inherit">{formatBytes(cacheStats.db_size_bytes)}</span></div>
                </div>
              </div>
            )}

            {loading && !cacheSettings && (
              <div className="flex items-center justify-center gap-2 py-4 text-sm text-zinc-500">
                <Loader2 size={16} className="animate-spin" /> 加载中...
              </div>
            )}

            {cacheSettings && (
              <>
                {/* Retention settings */}
                <div>
                  <div className="text-sm font-semibold">保留策略</div>
                  <div className="mt-3 space-y-4">
                    <div>
                      <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                        邮件正文保留天数: {cacheSettings.body_retention_days} 天
                      </label>
                      <input
                        type="range"
                        min={7} max={365} step={1}
                        value={cacheSettings.body_retention_days}
                        onChange={(e) => setCacheSet({ ...cacheSettings, body_retention_days: Number(e.target.value) })}
                        className="mt-1 w-full"
                      />
                      <div className={`flex justify-between text-xs ${dark ? "text-zinc-500" : "text-zinc-400"}`}>
                        <span>7 天</span><span>365 天</span>
                      </div>
                    </div>
                    <div>
                      <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                        附件最大占用: {cacheSettings.attachment_max_mb} MB
                      </label>
                      <input
                        type="range"
                        min={100} max={5000} step={100}
                        value={cacheSettings.attachment_max_mb}
                        onChange={(e) => setCacheSet({ ...cacheSettings, attachment_max_mb: Number(e.target.value) })}
                        className="mt-1 w-full"
                      />
                      <div className={`flex justify-between text-xs ${dark ? "text-zinc-500" : "text-zinc-400"}`}>
                        <span>100 MB</span><span>5000 MB</span>
                      </div>
                    </div>
                    <div>
                      <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                        总缓存上限: {cacheSettings.total_cache_max_mb} MB
                      </label>
                      <input
                        type="range"
                        min={500} max={10000} step={500}
                        value={cacheSettings.total_cache_max_mb}
                        onChange={(e) => setCacheSet({ ...cacheSettings, total_cache_max_mb: Number(e.target.value) })}
                        className="mt-1 w-full"
                      />
                      <div className={`flex justify-between text-xs ${dark ? "text-zinc-500" : "text-zinc-400"}`}>
                        <span>500 MB</span><span>10000 MB</span>
                      </div>
                    </div>
                  </div>
                  <button
                    onClick={() => void handleSaveCache()}
                    disabled={saving}
                    className="mt-4 rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600 disabled:opacity-50"
                  >
                    {saving ? "保存中..." : "保存设置"}
                  </button>
                </div>

                {/* Actions */}
                <div>
                  <div className="text-sm font-semibold">缓存操作</div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <button
                      onClick={() => void handleClearCache()}
                      disabled={saving}
                      className={`flex items-center gap-2 rounded-xl px-4 py-2 text-sm transition ${
                        dark ? "bg-red-500/20 text-red-300 hover:bg-red-500/30" : "bg-red-500/10 text-red-600 hover:bg-red-500/20"
                      } disabled:opacity-50`}
                    >
                      <Trash2 size={14} /> 清除附件缓存
                    </button>
                    <button
                      onClick={() => void handlePurge()}
                      disabled={saving}
                      className={`flex items-center gap-2 rounded-xl px-4 py-2 text-sm transition ${
                        dark ? "bg-yellow-500/20 text-yellow-300 hover:bg-yellow-500/30" : "bg-yellow-500/10 text-yellow-600 hover:bg-yellow-500/20"
                      } disabled:opacity-50`}
                    >
                      <RefreshCw size={14} /> 清理旧邮件
                    </button>
                  </div>
                </div>
              </>
            )}

            {actionMsg && (
              <div className={`rounded-xl px-4 py-3 text-sm ${
                actionMsg.includes("失败") 
                  ? (dark ? "bg-red-500/10 text-red-300" : "bg-red-500/10 text-red-600")
                  : (dark ? "bg-green-500/10 text-green-300" : "bg-green-500/10 text-green-600")
              }`}>
                {actionMsg}
              </div>
            )}
          </div>
        )}

        {/* Import Tab */}
        {activeTab === "import" && (
          <div className="mt-5 space-y-5">
            <div>
              <div className="text-sm font-semibold">导入 mbox / eml 邮件</div>
              <p className={`mt-2 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                支持从 Thunderbird、Outlook 等导出的 mbox 文件，或单个 .eml 文件导入邮件到本地缓存。
              </p>
            </div>

            {importAccounts.length > 0 ? (
              <>
                <div>
                  <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>目标账号</label>
                  <select
                    value={importAccountId}
                    onChange={(e) => void handleAccountChange(e.target.value)}
                    className={`mt-1 w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                      dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                    }`}
                  >
                    {importAccounts.map((a) => (
                      <option key={a.id} value={a.id}>{a.email}</option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>目标文件夹</label>
                  <select
                    value={importFolderPath}
                    onChange={(e) => setImportFolderPath(e.target.value)}
                    className={`mt-1 w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                      dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                    }`}
                  >
                    {importFolders.filter((f) => f.selectable).map((f) => (
                      <option key={f.path} value={f.path}>{f.name} ({f.path})</option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>文件路径</label>
                  <input
                    value={importFilePath}
                    onChange={(e) => setImportFilePath(e.target.value)}
                    placeholder="C:\Users\Wooxi\Desktop\mail.mbox 或 .eml"
                    className={`mt-1 w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                      dark ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500" : "border-black/10 bg-white text-black placeholder:text-zinc-400"
                    }`}
                  />
                </div>

                <button
                  onClick={() => void handleImport()}
                  disabled={importing || !importAccountId || !importFilePath.trim()}
                  className="rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600 disabled:opacity-50"
                >
                  {importing ? "导入中..." : "开始导入"}
                </button>
              </>
            ) : (
              <div className={`rounded-2xl border border-dashed p-6 text-center text-sm ${
                dark ? "border-white/10 text-zinc-500" : "border-black/10 text-zinc-600"
              }`}>
                请先添加邮箱账号才能导入邮件
              </div>
            )}

            {importMsg && (
              <div className={`rounded-xl px-4 py-3 text-sm ${
                importMsg.startsWith("成功")
                  ? dark ? "bg-green-500/10 text-green-300" : "bg-green-500/10 text-green-600"
                  : dark ? "bg-red-500/10 text-red-300" : "bg-red-500/10 text-red-600"
              }`}>{importMsg}</div>
            )}
          </div>
        )}

        {/* Rules Tab */}
        {activeTab === "rules" && (
          <div className="mt-5 space-y-5">
            <div>
              <div className="text-sm font-semibold">自动规则</div>
              <p className={`mt-2 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                新邮件同步后自动按规则打标签或移动到指定文件夹。
              </p>
            </div>

            {/* New rule form */}
            <div className={`rounded-2xl border p-4 ${dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"}`}>
              <div className="grid grid-cols-2 gap-2">
                <input value={ruleForm.name} onChange={(e) => setRuleForm({ ...ruleForm, name: e.target.value })} placeholder="规则名称"
                  className={`rounded-xl border px-3 py-2 text-sm outline-none ${dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"}`} />
                <select value={ruleForm.field} onChange={(e) => setRuleForm({ ...ruleForm, field: e.target.value })}
                  className={`rounded-xl border px-3 py-2 text-sm outline-none ${dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"}`}>
                  <option value="from">发件人</option>
                  <option value="subject">主题</option>
                  <option value="to">收件人</option>
                </select>
                <select value={ruleForm.operator} onChange={(e) => setRuleForm({ ...ruleForm, operator: e.target.value })}
                  className={`rounded-xl border px-3 py-2 text-sm outline-none ${dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"}`}>
                  <option value="contains">包含</option>
                  <option value="equals">等于</option>
                  <option value="starts_with">开头是</option>
                </select>
                <input value={ruleForm.value} onChange={(e) => setRuleForm({ ...ruleForm, value: e.target.value })} placeholder="匹配值"
                  className={`rounded-xl border px-3 py-2 text-sm outline-none ${dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"}`} />
                <select value={ruleForm.action_type} onChange={(e) => setRuleForm({ ...ruleForm, action_type: e.target.value })}
                  className={`rounded-xl border px-3 py-2 text-sm outline-none ${dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"}`}>
                  <option value="tag">打标签</option>
                  <option value="move">移动到</option>
                </select>
                <input value={ruleForm.action_value} onChange={(e) => setRuleForm({ ...ruleForm, action_value: e.target.value })} placeholder={ruleForm.action_type === "tag" ? "标签名" : "文件夹路径"}
                  className={`rounded-xl border px-3 py-2 text-sm outline-none ${dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"}`} />
              </div>
              <button onClick={() => void handleCreateRule()} disabled={ruleSaving}
                className="mt-3 rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600 disabled:opacity-50">
                {ruleSaving ? "创建中..." : "添加规则"}
              </button>
              {ruleMsg && <div className={`mt-2 text-sm ${ruleMsg.startsWith("失败") ? "text-red-400" : "text-green-400"}`}>{ruleMsg}</div>}
            </div>

            {/* Rule list */}
            {rules.length === 0 ? (
              <div className={`rounded-2xl border border-dashed p-6 text-center text-sm ${dark ? "border-white/10 text-zinc-500" : "border-black/10 text-zinc-600"}`}>
                还没有规则，添加一条来自动处理邮件
              </div>
            ) : (
              <div className="space-y-2">
                {rules.map((rule) => (
                  <div key={rule.id} className={`flex items-center gap-3 rounded-2xl border p-3 ${dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"}`}>
                    <input type="checkbox" checked={rule.enabled} onChange={(e) => void handleToggleRule(rule.id, e.target.checked)} />
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium truncate">{rule.name}</div>
                      <div className={`text-xs truncate ${dark ? "text-zinc-500" : "text-zinc-400"}`}>
                        {fieldLabel(rule.field)} {opLabel(rule.operator)} "{rule.value}" → {actionLabel(rule.action_type, rule.action_value)}
                      </div>
                    </div>
                    <button onClick={() => void handleDeleteRule(rule.id)}
                      className={`shrink-0 rounded-lg p-1.5 text-xs transition ${dark ? "text-red-400 hover:bg-red-500/20" : "text-red-500 hover:bg-red-500/10"}`}>
                      删除
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Translate Tab */}
        {activeTab === "translate" && (
          <div className="mt-5 space-y-5">
            <div>
              <div className="text-sm font-semibold">翻译设置</div>
              <p className={`mt-2 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                使用百度翻译 API 翻译邮件正文。
                <a href="https://fanyi-api.baidu.com/" target="_blank" className="text-blue-400 underline ml-1">免费获取密钥</a>
              </p>
            </div>
            <div className="space-y-3">
              <div>
                <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>APP ID</label>
                <input
                  value={translateAppid}
                  onChange={(e) => setTranslateAppid(e.target.value)}
                  placeholder="20260610002629505"
                  className={`mt-1 w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                    dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                  }`}
                />
              </div>
              <div>
                <label className={`text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>密钥 (Secret)</label>
                <input
                  value={translateSecret}
                  onChange={(e) => setTranslateSecret(e.target.value)}
                  placeholder="••••••••••••••••"
                  type="password"
                  className={`mt-1 w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                    dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                  }`}
                />
              </div>
              <button
                onClick={() => {
                  window.localStorage.setItem("woxmail.translateAppid", translateAppid)
                  window.localStorage.setItem("woxmail.translateSecret", translateSecret)
                  setTranslateSaved(true)
                  setTimeout(() => setTranslateSaved(false), 2000)
                }}
                className="rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600"
              >
                {translateSaved ? "已保存" : "保存密钥"}
              </button>
            </div>
          </div>
        )}

      </div>
    </div>
  )
}

export default SettingsModal
