import type { CacheSettings, CacheStats } from "../types/mail"
import { tauriInvoke } from "./tauri"

export async function getCacheSettings(): Promise<CacheSettings> {
  return tauriInvoke<CacheSettings>("get_cache_settings")
}

export async function setCacheSettings(input: CacheSettings): Promise<void> {
  await tauriInvoke<void>("set_cache_settings", { input })
}

export async function getCacheStats(): Promise<CacheStats> {
  return tauriInvoke<CacheStats>("get_cache_stats")
}

export async function clearCache(): Promise<number> {
  return tauriInvoke<number>("clear_cache")
}

export async function purgeOldMessages(olderThanDays: number): Promise<number> {
  return tauriInvoke<number>("purge_old_messages", { olderThanDays })
}
