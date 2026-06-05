import type { TFunction } from "i18next"

import type { MailFolderInfo } from "../types/mail"

type FolderDisplayInput = Pick<MailFolderInfo, "path" | "name"> | string

export function isSelectableFolder(folder: MailFolderInfo) {
  return folder.selectable
}

export function folderDisplayName(folder: FolderDisplayInput, t: TFunction) {
  const path = typeof folder === "string" ? folder : folder.path
  const name = typeof folder === "string" ? folder : folder.name || folder.path
  const value = `${path} ${name}`.toLowerCase()
  const compactValue = value.replace(/[\s_\-[\]/.]+/g, "")

  if (compactValue === "inbox" || value.includes("inbox")) return t("inbox")
  if (value.includes("sent") || value.includes("已发送") || value.includes("发件")) {
    return t("folder_sent")
  }
  if (value.includes("draft") || value.includes("草稿")) return t("folder_drafts")
  if (
    value.includes("trash") ||
    value.includes("deleted") ||
    value.includes("bin") ||
    value.includes("已删除") ||
    value.includes("废纸篓")
  ) {
    return t("folder_trash")
  }
  if (value.includes("spam") || value.includes("junk") || value.includes("垃圾邮件")) {
    return t("folder_junk")
  }
  if (value.includes("archive") || value.includes("归档")) return t("folder_archive")
  if (
    value.includes("all mail") ||
    compactValue.includes("allmail") ||
    value.includes("所有邮件")
  ) {
    return t("folder_all_mail")
  }

  return name || path
}
