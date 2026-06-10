import { useEffect } from "react"

export type ShortcutAction =
  | "compose"
  | "reply"
  | "replyAll"
  | "forward"
  | "delete"
  | "archive"
  | "search"
  | "nextMessage"
  | "prevMessage"
  | "send"
  | "close"
  | "refresh"

const defaultBindings: Record<ShortcutAction, { key: string; ctrl?: boolean; shift?: boolean; alt?: boolean }> = {
  compose:      { key: "n", ctrl: true },
  reply:        { key: "r", ctrl: true },
  replyAll:     { key: "r", ctrl: true, shift: true },
  forward:      { key: "f", ctrl: true },
  delete:       { key: "Delete", shift: true },
  archive:      { key: "Delete" },
  search:       { key: "k", ctrl: true },
  nextMessage:  { key: "j" },
  prevMessage:  { key: "k" },
  send:         { key: "Enter", ctrl: true },
  close:        { key: "Escape" },
  refresh:      { key: "r", ctrl: true, shift: true },
}

export type ShortcutHandler = (action: ShortcutAction) => void

export function useKeyboardShortcuts(
  onAction: ShortcutHandler,
  enabled = true,
) {
  useEffect(() => {
    if (!enabled) return

    const matchAction = (e: KeyboardEvent): ShortcutAction | null => {
      // Skip when typing in inputs/textarea/contenteditable
      const tag = (e.target as HTMLElement)?.tagName?.toLowerCase()
      const isContentEditable = (e.target as HTMLElement)?.isContentEditable
      if (tag === "input" || tag === "textarea" || tag === "select" || isContentEditable) {
        return null
      }

      for (const [action, binding] of Object.entries(defaultBindings)) {
        if (
          e.key.toLowerCase() === binding.key.toLowerCase() &&
          !!e.ctrlKey === !!binding.ctrl &&
          !!e.shiftKey === !!binding.shift &&
          !!e.altKey === !!binding.alt
        ) {
          return action as ShortcutAction
        }
      }
      return null
    }

    const handler = (e: KeyboardEvent) => {
      const action = matchAction(e)
      if (action) {
        e.preventDefault()
        e.stopPropagation()
        onAction(action)
      }
    }

    window.addEventListener("keydown", handler)
    return () => window.removeEventListener("keydown", handler)
  }, [onAction, enabled])
}

export function getShortcutLabel(action: ShortcutAction): string {
  const b = defaultBindings[action]
  const parts: string[] = []
  if (b.ctrl) parts.push("Ctrl")
  if (b.shift) parts.push("Shift")
  if (b.alt) parts.push("Alt")
  const keyLabel = b.key === "Delete" ? "Del" : b.key === "Escape" ? "Esc" : b.key.toUpperCase()
  parts.push(keyLabel)
  return parts.join("+")
}

export const shortcutDescriptions: Record<ShortcutAction, string> = {
  compose:     "写新邮件",
  reply:       "回复",
  replyAll:    "全部回复",
  forward:     "转发",
  delete:      "永久删除",
  archive:     "删除/归档",
  search:      "搜索",
  nextMessage: "下一封",
  prevMessage: "上一封",
  send:        "发送",
  close:       "关闭/取消",
  refresh:     "刷新同步",
}
