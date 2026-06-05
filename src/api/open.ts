import { openUrl } from "@tauri-apps/plugin-opener"
import { isTauriRuntime } from "./tauri"

export async function openExternalUrl(url: string) {
  if (isTauriRuntime()) {
    await openUrl(url)
    return
  }

  window.open(url, "_blank", "noopener,noreferrer")
}
