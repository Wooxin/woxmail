import i18n from "i18next"

import { initReactI18next } from "react-i18next"

const resources = {
	en: {
		translation: {
			welcome: "Welcome to Wox Mail",
			add_mailbox:
				"Add your first mailbox to get started",

			add_account: "Add Account",

			accounts: "Accounts",

			no_accounts: "No accounts",

			choose_provider:
				"Choose your email provider",

			inbox: "Inbox",

			folders: "Folders",

			folder_sent: "Sent",

			folder_drafts: "Drafts",

			folder_trash: "Trash",

			folder_junk: "Junk",

			folder_archive: "Archive",

			folder_all_mail: "All Mail",
		},
	},

	zh: {
		translation: {
			welcome: "欢迎使用 Wox Mail",

			add_mailbox:
				"添加你的第一个邮箱账户",

			add_account: "添加账户",

			accounts: "账户",

			no_accounts: "暂无账户",

			choose_provider:
				"选择你的邮箱服务商",

			inbox: "收件箱",

			folders: "文件夹",

			folder_sent: "已发送",

			folder_drafts: "草稿箱",

			folder_trash: "已删除",

			folder_junk: "垃圾邮件",

			folder_archive: "归档",

			folder_all_mail: "所有邮件",
		},
	},

	ja: {
		translation: {
			welcome:
				"Wox Mail へようこそ",

			add_mailbox:
				"最初のメールアカウントを追加してください",

			add_account:
				"アカウント追加",

			accounts: "アカウント",

			no_accounts:
				"アカウントがありません",

			choose_provider:
				"メールプロバイダーを選択",

			inbox: "受信トレイ",

			folders: "フォルダー",

			folder_sent: "送信済み",

			folder_drafts: "下書き",

			folder_trash: "ゴミ箱",

			folder_junk: "迷惑メール",

			folder_archive: "アーカイブ",

			folder_all_mail: "すべてのメール",
		},
	},
}

const systemLang =
	navigator.language.startsWith("zh")
		? "zh"
		: navigator.language.startsWith("ja")
			? "ja"
			: "en"

i18n.use(initReactI18next).init({
	resources,

	lng: systemLang,

	fallbackLng: "en",

	interpolation: {
		escapeValue: false,
	},
})

export default i18n
