import { X } from "lucide-react"

type SidebarAccountMode = "list" | "dropdown"

type Props = {
  dark: boolean
  sidebarAccountMode: SidebarAccountMode
  onSidebarAccountModeChange: (mode: SidebarAccountMode) => void
  onClose: () => void
}

function SettingsModal({
  dark,
  sidebarAccountMode,
  onSidebarAccountModeChange,
  onClose,
}: Props) {
  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-md">
      <div
        className={`w-[560px] rounded-3xl border p-6 shadow-2xl ${
          dark ? "border-white/10 bg-[#111]" : "border-black/10 bg-white"
        }`}
      >
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold">设置</h2>
            <p className={`mt-1 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
              基础显示和登录方式
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

        <div className="mt-6">
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
                type="radio"
                name="sidebarAccountMode"
                className="mt-1"
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
                type="radio"
                name="sidebarAccountMode"
                className="mt-1"
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

        <div className="mt-6">
          <div className="text-sm font-semibold">Gmail OAuth</div>
          <div className={`mt-2 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
            Gmail 可以直接弹出浏览器授权登录，不需要用户配置开发者参数。
          </div>
        </div>

        <div className={`mt-5 rounded-2xl border p-4 text-sm ${
          dark ? "border-white/10 bg-white/[0.03] text-zinc-400" : "border-black/10 bg-black/[0.03] text-zinc-600"
        }`}>
          Gmail OAuth 使用系统浏览器完成授权，token 会保存到本机安全存储；普通 IMAP 邮箱仍使用授权码。
        </div>
      </div>
    </div>
  )
}

export default SettingsModal
