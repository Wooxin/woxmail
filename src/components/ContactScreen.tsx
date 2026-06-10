import { useEffect, useState } from "react"
import { Search, Plus, Trash2, User, Mail, Phone, Pencil, Download, Loader2 } from "lucide-react"
import type { Contact, CreateContactInput } from "../types/mail"
import { listContacts, createContact, updateContact, deleteContact, importContacts } from "../api/contact"

type Props = { dark: boolean }

function ContactScreen({ dark }: Props) {
  const [contacts, setContacts] = useState<Contact[]>([])
  const [search, setSearch] = useState("")
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [editing, setEditing] = useState<Contact | null>(null)
  const [showNew, setShowNew] = useState(false)
  const [form, setForm] = useState<CreateContactInput>({ name: "", email: "", phone: "", notes: "" })
  const [saving, setSaving] = useState(false)
  const [importing, setImporting] = useState(false)

  const load = async (query?: string) => {
    setLoading(true)
    setError(null)
    try {
      setContacts(await listContacts(query))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { void load() }, [])

  useEffect(() => {
    const t = window.setTimeout(() => void load(search || undefined), 300)
    return () => window.clearTimeout(t)
  }, [search])

  const resetForm = () => {
    setForm({ name: "", email: "", phone: "", notes: "" })
    setEditing(null)
    setShowNew(false)
  }

  const handleSave = async () => {
    if (!form.name.trim() || !form.email.trim()) return
    setSaving(true)
    setError(null)
    try {
      if (editing) {
        await updateContact(editing.id, form)
      } else {
        await createContact(form)
      }
      resetForm()
      await load(search || undefined)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const handleEdit = (c: Contact) => {
    setEditing(c)
    setShowNew(false)
    setForm({ name: c.name, email: c.email, phone: c.phone ?? "", notes: c.notes ?? "" })
  }

  const handleDelete = async (c: Contact) => {
    if (!window.confirm(`删除联系人 ${c.name}？`)) return
    setError(null)
    try {
      await deleteContact(c.id)
      if (editing?.id === c.id) resetForm()
      await load(search || undefined)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const handleImport = async () => {
    setImporting(true)
    setError(null)
    try {
      const count = await importContacts()
      await load()
      setError(`已从邮件中导入 ${count} 个联系人`)
      setTimeout(() => setError(null), 3000)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setImporting(false)
    }
  }

  const showForm = showNew || !!editing

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className={`flex shrink-0 items-center justify-between border-b p-5 ${dark ? "border-white/10" : "border-black/10"}`}>
        <h2 className="text-lg font-semibold">通讯录</h2>
        <div className="flex gap-2">
          <button
            onClick={() => void handleImport()}
            disabled={importing}
            className={`flex items-center gap-2 rounded-xl px-4 py-2 text-sm transition disabled:opacity-50 ${
              dark ? "bg-white/10 text-zinc-200 hover:bg-white/15" : "bg-black/10 text-zinc-700 hover:bg-black/15"
            }`}
          >
            {importing ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
            从邮件导入
          </button>
          <button
            onClick={() => { resetForm(); setShowNew(true) }}
            className="flex items-center gap-2 rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600"
          >
            <Plus size={16} /> 新建
          </button>
        </div>
      </div>

      {/* Search */}
      <div className={`shrink-0 border-b px-5 py-3 ${dark ? "border-white/10" : "border-black/10"}`}>
        <div className={`flex items-center gap-2 rounded-xl border px-3 py-2 ${
          dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"
        }`}>
          <Search size={16} className={dark ? "text-zinc-500" : "text-zinc-400"} />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索联系人..."
            className={`flex-1 bg-transparent text-sm outline-none ${dark ? "text-white placeholder:text-zinc-500" : "text-black placeholder:text-zinc-400"}`}
          />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {error && (
          <div className={`m-4 rounded-xl px-4 py-3 text-sm ${
            error.includes("已从邮件")
              ? dark ? "bg-green-500/10 text-green-300" : "bg-green-500/10 text-green-600"
              : dark ? "bg-red-500/10 text-red-300" : "bg-red-500/10 text-red-600"
          }`}>{error}</div>
        )}

        {/* Edit / New Form */}
        {showForm && (
          <div className={`m-4 rounded-2xl border p-4 ${dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"}`}>
            <h3 className="mb-3 font-medium">{editing ? "编辑联系人" : "新建联系人"}</h3>
            <div className="space-y-3">
              <input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="姓名"
                className={`w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                  dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                }`}
              />
              <input
                value={form.email}
                onChange={(e) => setForm({ ...form, email: e.target.value })}
                placeholder="邮箱"
                type="email"
                className={`w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                  dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                }`}
              />
              <input
                value={form.phone ?? ""}
                onChange={(e) => setForm({ ...form, phone: e.target.value })}
                placeholder="电话 (选填)"
                className={`w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                  dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                }`}
              />
              <textarea
                value={form.notes ?? ""}
                onChange={(e) => setForm({ ...form, notes: e.target.value })}
                placeholder="备注 (选填)"
                rows={2}
                className={`w-full rounded-xl border px-3 py-2 text-sm outline-none ${
                  dark ? "border-white/10 bg-white/5 text-white" : "border-black/10 bg-white text-black"
                }`}
              />
              <div className="flex gap-2">
                <button
                  onClick={() => void handleSave()}
                  disabled={saving || !form.name.trim() || !form.email.trim()}
                  className="rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600 disabled:opacity-50"
                >
                  {saving ? "保存中..." : "保存"}
                </button>
                <button
                  onClick={resetForm}
                  className={`rounded-xl px-4 py-2 text-sm transition ${
                    dark ? "bg-white/10 hover:bg-white/15" : "bg-black/10 hover:bg-black/15"
                  }`}
                >
                  取消
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Contact List */}
        {loading ? (
          <div className={`flex items-center justify-center p-12 ${dark ? "text-zinc-500" : "text-zinc-600"}`}>
            <Loader2 size={24} className="animate-spin" />
          </div>
        ) : contacts.length === 0 ? (
          <div className={`flex flex-col items-center justify-center p-12 ${dark ? "text-zinc-500" : "text-zinc-600"}`}>
            <User size={48} className="mb-4 opacity-30" />
            <div className="text-lg">暂无联系人</div>
            <div className="mt-1 text-sm">点击"从邮件导入"或"新建"添加联系人</div>
          </div>
        ) : (
          <div className="divide-y">
            {contacts.map((c) => (
              <div
                key={c.id}
                className={`flex items-center gap-4 p-4 transition ${
                  editing?.id === c.id
                    ? dark ? "bg-blue-500/10" : "bg-blue-500/5"
                    : dark ? "hover:bg-white/5" : "hover:bg-black/5"
                }`}
              >
                <div className={`flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-lg font-bold ${
                  dark ? "bg-white/10 text-zinc-300" : "bg-black/10 text-zinc-600"
                }`}>
                  {c.name.charAt(0).toUpperCase()}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="font-medium truncate">{c.name}</div>
                  <div className={`flex items-center gap-3 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                    <span className="flex items-center gap-1"><Mail size={12} /> {c.email}</span>
                    {c.phone && <span className="flex items-center gap-1"><Phone size={12} /> {c.phone}</span>}
                  </div>
                  {c.notes && <div className={`mt-0.5 truncate text-xs ${dark ? "text-zinc-500" : "text-zinc-400"}`}>{c.notes}</div>}
                </div>
                <div className="flex shrink-0 gap-1">
                  <button
                    onClick={() => handleEdit(c)}
                    className={`rounded-lg p-2 transition ${dark ? "hover:bg-white/10 text-zinc-400" : "hover:bg-black/10 text-zinc-500"}`}
                  >
                    <Pencil size={14} />
                  </button>
                  <button
                    onClick={() => void handleDelete(c)}
                    className={`rounded-lg p-2 transition ${dark ? "hover:bg-red-500/20 text-zinc-400" : "hover:bg-red-500/10 text-zinc-500"}`}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

export default ContactScreen
