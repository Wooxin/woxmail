import { useState } from "react"
import { Minus, Square, X, Moon, Sun, Languages, Settings } from "lucide-react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useTranslation } from "react-i18next"
import { isTauriRuntime } from "../api/tauri"

type Props = {
  dark: boolean
  setDark: (v: boolean) => void
  onSettings: () => void
}

function TitleBar({ dark, setDark, onSettings }: Props) {
  const { i18n } = useTranslation()
  const [showLangMenu, setShowLangMenu] = useState(false)

  const changeLang = (lang: string) => {
    i18n.changeLanguage(lang)
    setShowLangMenu(false)
  }

  const withAppWindow = async (
    action: (appWindow: ReturnType<typeof getCurrentWindow>) => Promise<void>,
  ) => {
    if (!isTauriRuntime()) return
    await action(getCurrentWindow())
  }

  return (
    <div data-tauri-drag-region className={`flex h-12 items-center justify-between border-b px-4 ${dark ? "border-white/10 bg-[#09090B] text-white" : "border-black/10 bg-white text-black"}`}>
      <div className="font-medium" data-tauri-drag-region>Wox Mail</div>

      <div className="flex items-center gap-1 relative">
        {/* 语言切换按钮 */}
        <button 
          onClick={() => setShowLangMenu(!showLangMenu)} 
          className={`relative rounded-lg p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}
        >
          <Languages size={16} />
        </button>

        {/* 二级菜单 */}
        {showLangMenu && (
          <div className={`absolute right-16 top-10 w-32 rounded-xl border p-1 shadow-xl z-50 ${dark ? "border-white/10 bg-[#18181b]" : "border-black/10 bg-white"}`}>
            {["en", "zh", "ja"].map((lang) => (
              <button
                key={lang}
                onClick={() => changeLang(lang)}
                className={`w-full rounded-lg px-3 py-2 text-left text-sm transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}
              >
                {lang.toUpperCase()}
              </button>
            ))}
          </div>
        )}

        <button onClick={() => setDark(!dark)} className={`rounded-lg p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}>
          {dark ? <Sun size={16} /> : <Moon size={16} />}
        </button>
        <button
          title="设置"
          onClick={onSettings}
          className={`rounded-lg p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}
        >
          <Settings size={16} />
        </button>
        <button onClick={() => void withAppWindow((appWindow) => appWindow.minimize())} className={`rounded-lg p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}><Minus size={16} /></button>
        <button onClick={() => void withAppWindow((appWindow) => appWindow.toggleMaximize())} className={`rounded-lg p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}><Square size={14} /></button>
        <button onClick={() => void withAppWindow((appWindow) => appWindow.hide())} className="rounded-lg p-2 transition hover:bg-red-500 hover:text-white"><X size={16} /></button>
      </div>
    </div>
  )
}

export default TitleBar
