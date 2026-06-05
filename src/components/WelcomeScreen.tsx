import { Mail } from "lucide-react"
import { useTranslation } from "react-i18next"

type Props = {
  dark: boolean
  onAdd: () => void
}

function WelcomeScreen({ dark, onAdd }: Props) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-1 items-center justify-center p-10">
      <div className="max-w-2xl text-center">
        <div className="mb-6 flex justify-center">
          <div className="rounded-3xl bg-blue-500/20 p-6">
            <Mail size={64} />
          </div>
        </div>

        <h1 className="mb-6 text-6xl font-bold">{t("welcome")}</h1>

        <p className={`mb-10 text-xl ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
          {t("add_mailbox")}
        </p>

        <button
          onClick={onAdd}
          className="rounded-2xl bg-white px-8 py-4 text-lg font-medium text-black transition hover:scale-[1.02]"
        >
          {t("add_account")}
        </button>
      </div>
    </div>
  )
}

export default WelcomeScreen