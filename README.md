# Wox Mail

高性能本地桌面邮箱客户端，替代 Thunderbird。

基于 **Tauri 2** (Rust) + **React 19** (TypeScript)，数据本地加密存储，便携不污染系统盘。

## 功能

- 📧 **多账户** — Gmail/Outlook OAuth + QQ/163/iCloud/Proton Bridge/自定义 IMAP/SMTP
- 🔒 **安全 HTML 渲染** — 白名单过滤，默认阻止远程图片和脚本
- 🔍 **全文搜索** — SQLite FTS5，支持中文
- 📇 **通讯录** — 管理联系人，自动从邮件导入，写信时自动补全
- 🌐 **邮件翻译** — 百度翻译 API（设置中自配密钥）
- 📋 **邮件规则** — 按发件人/主题/收件人自动打标签或移动
- 🧵 **会话视图** — 按主题自动分组线程
- 📥 **导入** — 支持 mbox/eml 导入迁移
- ⌨️ **快捷键** — Ctrl+N 写信 / Ctrl+R 回复 / J/K 导航等
- 📤 **发件箱管理** — 失败重试、手动取消
- 💾 **便携模式** — 所有数据存储在 exe 同级 `woxmail-data/` 目录

## 系统要求

- Windows 10/11 (x64)
- Visual C++ 运行库

## 开发

```bash
# 安装依赖
npm install

# 开发模式
npm run tauri dev

# 构建
npm run tauri build
```

> 需要 VS x64 Native Tools Command Prompt 或安装 Visual Studio Build Tools

## 技术栈

| 层级 | 技术 |
|------|------|
| 框架 | Tauri 2 (Rust) |
| 前端 | React 19 + TypeScript + Vite |
| 数据库 | SQLite (rusqlite + FTS5) |
| 邮件协议 | IMAP (imap crate) + SMTP (lettre) |
| UI 图标 | Lucide React |

## 许可证

MIT
