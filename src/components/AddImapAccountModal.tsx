import { useState } from "react"
import { X } from "lucide-react"

type Props = {
  dark: boolean
  busy?: boolean
  error?: string | null
  providerName?: string
  initialValues?: Partial<ImapAccountInput>
  loginHint?: string
  passwordLabel?: string
  onClose: () => void
  onSubmit: (input: ImapAccountInput) => void
}

export type ImapAccountInput = {
  name: string
  email: string
  imapHost: string
  imapPort: number
  imapTls: boolean
  imapUsername: string
  smtpHost: string
  smtpPort: number
  smtpTls: boolean
  smtpUsername: string
  password: string
}

function AddImapAccountModal({
  dark,
  busy = false,
  error,
  providerName = "Custom IMAP",
  initialValues,
  loginHint,
  passwordLabel = "密码 / 授权码",
  onClose,
  onSubmit,
}: Props) {
  const [name, setName] = useState(initialValues?.name ?? "")
  const [email, setEmail] = useState(initialValues?.email ?? "")
  const [password, setPassword] = useState("")

  const [imapHost, setImapHost] = useState(initialValues?.imapHost ?? "")
  const [imapPort, setImapPort] = useState(initialValues?.imapPort ?? 993)
  const [imapTls, setImapTls] = useState(initialValues?.imapTls ?? true)
  const [imapUsername, setImapUsername] = useState(initialValues?.imapUsername ?? "")

  const [smtpHost, setSmtpHost] = useState(initialValues?.smtpHost ?? "")
  const [smtpPort, setSmtpPort] = useState(initialValues?.smtpPort ?? 465)
  const [smtpTls, setSmtpTls] = useState(initialValues?.smtpTls ?? true)
  const [smtpUsername, setSmtpUsername] = useState(initialValues?.smtpUsername ?? "")

  const isValidPort = (value: number) =>
    Number.isInteger(value) && value > 0 && value <= 65535

  const emailLooksValid = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim())

  const canSubmit = Boolean(
    !busy &&
    emailLooksValid &&
    email.trim() &&
    password.trim() &&
    imapHost.trim() &&
    isValidPort(imapPort) &&
    imapUsername.trim() &&
    smtpHost.trim() &&
    isValidPort(smtpPort) &&
    smtpUsername.trim(),
  )

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md">
      <div
        className={`max-h-[90vh] w-[760px] overflow-y-auto rounded-3xl border p-8 shadow-2xl ${
          dark ? "border-white/10 bg-[#111]" : "border-black/10 bg-white"
        }`}
      >
        <div className="mb-6 flex items-center justify-between">
          <div>
            <h2 className="text-3xl font-bold">{providerName}</h2>
            <p className={`mt-2 ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              {loginHint ?? "使用 IMAP/SMTP 登录，密码或授权码会保存到系统钥匙串"}
            </p>
          </div>

          <button
            onClick={onClose}
            disabled={busy}
            className={`rounded-xl p-2 transition ${
              dark ? "hover:bg-white/10" : "hover:bg-black/10"
            }`}
          >
            <X size={20} />
          </button>
        </div>

        {error && (
          <div className="mb-4 rounded-2xl border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-300">
            {error}
          </div>
        )}

        <div className="grid grid-cols-2 gap-4">
          <div className="col-span-2 grid grid-cols-2 gap-4">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="显示名称（可选）"
              className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
                dark
                  ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                  : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
              }`}
            />
            <input
              value={email}
              onChange={(e) => {
                const nextEmail = e.target.value
                setEmail(nextEmail)
                setImapUsername((current) =>
                  current.trim() && current !== email ? current : nextEmail,
                )
                setSmtpUsername((current) =>
                  current.trim() && current !== email ? current : nextEmail,
                )
              }}
              placeholder="邮箱地址"
              className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
                dark
                  ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                  : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
              }`}
            />
          </div>

          <input
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder={passwordLabel}
            type="password"
            className={`col-span-2 w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />

          <div className="col-span-2 mt-2 text-sm font-semibold">IMAP</div>

          <input
            value={imapHost}
            onChange={(e) => setImapHost(e.target.value)}
            placeholder="IMAP Host（例如 imap.qq.com）"
            className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />
          <input
            value={imapUsername}
            onChange={(e) => setImapUsername(e.target.value)}
            placeholder="IMAP Username（通常是邮箱地址）"
            className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />

          <input
            value={imapPort}
            onChange={(e) => setImapPort(Number(e.target.value || 0))}
            placeholder="IMAP Port"
            type="number"
            className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />

          <label
            className={`flex items-center gap-3 rounded-2xl border px-4 py-3 text-sm ${
              dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"
            }`}
          >
            <input
              checked={imapTls}
              onChange={(e) => setImapTls(e.target.checked)}
              type="checkbox"
            />
            TLS
          </label>

          <div className="col-span-2 mt-2 text-sm font-semibold">SMTP</div>

          <input
            value={smtpHost}
            onChange={(e) => setSmtpHost(e.target.value)}
            placeholder="SMTP Host（例如 smtp.qq.com）"
            className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />
          <input
            value={smtpUsername}
            onChange={(e) => setSmtpUsername(e.target.value)}
            placeholder="SMTP Username（通常是邮箱地址）"
            className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />

          <input
            value={smtpPort}
            onChange={(e) => setSmtpPort(Number(e.target.value || 0))}
            placeholder="SMTP Port"
            type="number"
            className={`w-full rounded-2xl border px-4 py-3 text-sm outline-none ${
              dark
                ? "border-white/10 bg-white/5 text-white placeholder:text-zinc-500"
                : "border-black/10 bg-black/5 text-black placeholder:text-zinc-500"
            }`}
          />

          <label
            className={`flex items-center gap-3 rounded-2xl border px-4 py-3 text-sm ${
              dark ? "border-white/10 bg-white/5" : "border-black/10 bg-black/5"
            }`}
          >
            <input
              checked={smtpTls}
              onChange={(e) => setSmtpTls(e.target.checked)}
              type="checkbox"
            />
            TLS
          </label>
        </div>

        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onClose}
            disabled={busy}
            className={`rounded-xl px-4 py-2 text-sm transition ${
              dark ? "hover:bg-white/10" : "hover:bg-black/10"
            }`}
          >
            取消
          </button>
          <button
            disabled={!canSubmit}
            onClick={() =>
              onSubmit({
                name,
                email,
                imapHost,
                imapPort,
                imapTls,
                imapUsername,
                smtpHost,
                smtpPort,
                smtpTls,
                smtpUsername,
                password,
              })
            }
            className={`rounded-xl px-4 py-2 text-sm font-medium transition ${
              canSubmit ? "bg-white text-black hover:scale-[1.02]" : "bg-zinc-500/30 text-zinc-400"
            }`}
          >
            {busy ? "保存中..." : "保存"}
          </button>
        </div>
      </div>
    </div>
  )
}

export default AddImapAccountModal
