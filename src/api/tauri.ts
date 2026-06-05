import { invoke } from "@tauri-apps/api/core"

export function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
}

export async function tauriInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauriRuntime()) {
    throw new Error("当前浏览器预览无法访问 Tauri 后端，请在 Tauri 应用中使用此功能")
  }

  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    const msg =
      e instanceof Error
        ? e.message
        : typeof e === "string"
          ? e
          : JSON.stringify(e)
    throw new Error(msg)
  }
}
