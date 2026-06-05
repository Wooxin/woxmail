import { Globe, KeyRound, Mail, Shield, X } from "lucide-react"
import type { MailProvider } from "../types/mail"

export type AddAccountLoginMode = "custom" | "oauth"

type Props = {
  dark: boolean
  busy?: boolean
  error?: string | null
  onClose: () => void
  onSelect: (provider: MailProvider, mode: AddAccountLoginMode) => void
}

const oauthProviders = [
  {
    id: "gmail",
    name: "Gmail",
    desc: "浏览器授权登录",
    icon: Mail,
  },
  {
    id: "outlook",
    name: "Outlook",
    desc: "浏览器授权登录",
    icon: Globe,
  },
] as const

const customProviders = [
  {
    id: "gmail",
    name: "Gmail",
    desc: "账号 + 应用专用密码",
    icon: KeyRound,
  },
  {
    id: "outlook",
    name: "Outlook",
    desc: "账号密码登录通常不可用",
    icon: Globe,
    disabled: true,
  },
  {
    id: "qq",
    name: "QQ Mail",
    desc: "账号 + 授权码",
    icon: Mail,
  },
  {
    id: "netease",
    name: "163 / 126",
    desc: "账号 + 授权码",
    icon: Mail,
  },
  {
    id: "icloud",
    name: "iCloud",
    desc: "账号 + App 密码",
    icon: Shield,
  },
  {
    id: "proton",
    name: "Proton",
    desc: "Bridge 账号密码",
    icon: Shield,
  },
  {
    id: "imap",
    name: "自定义 IMAP",
    desc: "手动填写服务器",
    icon: Globe,
  },
] as const

function AddAccountModal({
  dark,
  busy = false,
  error,
  onClose,
  onSelect,
}: Props) {
  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md">
      <div
        className={`w-[760px] rounded-2xl border p-6 shadow-2xl ${
          dark ? "border-white/10 bg-[#111]" : "border-black/10 bg-white"
        }`}
      >
        <div className="mb-5 flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold">添加邮箱账户</h2>
            <p className={`mt-2 ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              选择自定登录或快捷登录
            </p>
          </div>

          <button
            onClick={onClose}
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

        <div>
          <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
            <Globe size={16} />
            快捷登录（OAuth 登录）
          </div>
          <div className="grid grid-cols-3 gap-3">
          {oauthProviders.map((provider) => {
            const Icon = provider.icon
            const id = provider.id as MailProvider
            const disabled = busy || ("disabled" in provider ? provider.disabled === true : false)

            return (
              <div
                key={provider.id}
                className={`rounded-2xl border p-4 transition ${
                  dark
                    ? "border-white/10 bg-white/5"
                    : "border-black/10 bg-black/5"
                } ${disabled ? "opacity-60" : ""}`}
              >
                <div className="flex items-center gap-3">
                  <div className={`rounded-xl p-2 ${dark ? "bg-white/10" : "bg-black/10"}`}>
                    <Icon size={20} />
                  </div>
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold">{provider.name}</div>
                    <div className={`truncate text-xs ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                      {provider.desc}
                    </div>
                  </div>
                </div>
                <div className="mt-3">
                  <button
                    disabled={disabled}
                    onClick={() => onSelect(id, "oauth")}
                    className={`w-full rounded-lg px-2 py-1.5 text-xs transition ${
                      disabled
                        ? dark
                          ? "bg-white/5 text-zinc-600"
                          : "bg-black/5 text-zinc-400"
                        : dark
                          ? "bg-white/10 hover:bg-white/15"
                          : "bg-black/10 hover:bg-black/15"
                    }`}
                  >
                    {disabled ? "暂不可用" : "登录"}
                  </button>
                </div>
              </div>
            )
          })}
          </div>
        </div>

        <div className="mt-6">
          <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
            <KeyRound size={16} />
            自定登录（账号密码）
          </div>
          <div className="grid grid-cols-3 gap-3">
          {customProviders.map((provider) => {
            const Icon = provider.icon
            const id = provider.id as MailProvider
            const disabled = busy || ("disabled" in provider ? provider.disabled === true : false)

            return (
              <div
                key={provider.id}
                className={`rounded-2xl border p-4 transition ${
                  dark
                    ? "border-white/10 bg-white/5"
                    : "border-black/10 bg-black/5"
                } ${disabled ? "opacity-60" : ""}`}
              >
                <div className="flex items-center gap-3">
                  <div className={`rounded-xl p-2 ${dark ? "bg-white/10" : "bg-black/10"}`}>
                    <Icon size={20} />
                  </div>
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold">{provider.name}</div>
                    <div className={`truncate text-xs ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                      {provider.desc}
                    </div>
                  </div>
                </div>
                <div className="mt-3">
                  <button
                    disabled={disabled}
                    onClick={() => onSelect(id, "custom")}
                    className={`w-full rounded-lg px-2 py-1.5 text-xs transition ${
                      disabled
                        ? dark
                          ? "bg-white/5 text-zinc-600"
                          : "bg-black/5 text-zinc-400"
                        : dark
                          ? "bg-white/10 hover:bg-white/15"
                          : "bg-black/10 hover:bg-black/15"
                    }`}
                  >
                    {disabled ? "暂不可用" : "登录"}
                  </button>
                </div>
              </div>
            )
          })}
          </div>
        </div>
      </div>
    </div>
  )
}

export default AddAccountModal
