# Wox Mail — Thunderbird 替代全面升级计划

> **For Hermes:** 按 task 顺序依次实施，每完成一个 task 都要验证后继续。

**目标:** 将 Wox Mail 从基础邮箱客户端升级为 Thunderbird 级别的日用替代品，同时新增通讯录和 Bing 翻译功能。

**技术栈:** Tauri 2 (Rust) + React 19 + TypeScript + SQLite (rusqlite+FTS5)

---

## 阶段 1: P0 日用可靠性

### Task 1.1: 可视化发件箱管理界面

**目标:** 在侧边栏增加"发件箱"入口，显示失败/待发送邮件队列，支持手动重试/取消/编辑。

**文件:**
- 新建: `src/components/OutboxScreen.tsx`
- 修改: `src/App.tsx` — 增加发件箱状态和路由
- 修改: `src/components/Sidebar.tsx` — 增加发件箱入口
- 修改: `src/api/mail.ts` — 增加 `listOutboxJobs`, `retryOutboxJob`, `cancelOutboxJob` API
- 修改: `src-tauri/src/commands/mod.rs` — 增加对应 Tauri commands
- 修改: `src-tauri/src/models.rs` — OutboxJob 已存在，无需改动
- 修改: `src/types/mail.ts` — 增加 OutboxJob 类型

**Step 1:** 在 `src/types/mail.ts` 添加 `OutboxJob` 类型
```typescript
export interface OutboxJob {
  id: string
  account_id: string
  to_emails: string[]
  subject: string
  body: string
  is_html: boolean
  sent_folder_path: string
  status: string  // "pending" | "sending" | "failed"
  attempts: number
  last_error: string | null
  next_attempt_at: number
  created_at: number
  updated_at: number
}
```

**Step 2:** 在 Rust 后端添加 3 个新 commands:
- `list_outbox_jobs` — 查询所有 outbox_jobs，按状态排序
- `retry_outbox_job(job_id)` — 将 next_attempt_at 设为 now
- `cancel_outbox_job(job_id)` — 删除 job

**Step 3:** 在前端 `src/api/mail.ts` 添加对应 API 函数

**Step 4:** 创建 `src/components/OutboxScreen.tsx`，显示：
- 待发送列表（状态标签、收件人、主题、错误信息、重试按钮、取消按钮）
- 空状态提示

**Step 5:** 在 `Sidebar.tsx` 底部增加"发件箱"入口（带待发送数量 badge）

**Step 6:** 在 `App.tsx` 集成 OutboxScreen

---

### Task 1.2: 安全 HTML 邮件渲染

**目标:** 支持安全渲染 HTML 邮件正文，默认阻止远程图片和脚本，提供"加载远程内容"按钮。

**文件:**
- 新建: `src/components/HtmlRenderer.tsx`
- 修改: `src/components/MailScreen.tsx` — 使用 HtmlRenderer 替代纯文本显示
- 修改: `src-tauri/src/mail/mod.rs` — 保留 body_html 字段
- 修改: `src-tauri/src/db/mod.rs` — messages 表增加 body_html 列
- 修改: `src-tauri/src/models.rs` — MessageDetail 增加 body_html 和 is_html 字段
- 修改: `src/types/mail.ts` — MessageDetail 增加 body_html 和 is_html

**Step 1:** 数据库迁移：`messages` 表增加 `body_html TEXT` 列

**Step 2:** Rust models 增加字段：`MessageDetail.body_html: Option<String>`, `MessageDetail.is_html: bool`

**Step 3:** IMAP 同步时同时保存 HTML 正文（BODY.PEEK[] 拉取后按 MIME 类型分支存储）

**Step 4:** 创建 `HtmlRenderer.tsx` 组件：
- 使用 `dangerouslySetInnerHTML` + DOMPurify 白名单过滤
- 默认移除所有 `<img>` 的 `src`（用占位符替代）
- 禁止 `<script>`, `<iframe>`, `<object>`, `<embed>`
- 允许基本样式（字体、颜色、边距）
- 提供"加载远程内容"按钮重新渲染带图片的 HTML

**Step 5:** 在 MailScreen 阅读区使用 HtmlRenderer（有 body_html 时显示 HTML，否则回落纯文本）

**Step 6:** 安装 DOMPurify: `npm install dompurify && npm install @types/dompurify -D`

---

### Task 1.3: 缓存策略设置

**目标:** 在设置界面增加缓存管理，允许限制本地正文保存天数、附件大小和总缓存占用。

**文件:**
- 修改: `src/components/SettingsModal.tsx` — 增加缓存设置 Tab
- 修改: `src/types/mail.ts` — 增加 CacheSettings 类型
- 新建: `src/api/cache.ts` — cache API 函数
- 修改: `src-tauri/src/commands/mod.rs` — 增加 get_cache_stats, clear_cache, set_cache_policy commands
- 修改: `src-tauri/src/db/mod.rs` — cache_settings 表

**Step 1:** 数据库：`cache_settings` 表（body_retention_days: 30, attachment_max_mb: 500, total_cache_max_mb: 2000）

**Step 2:** Rust commands: `get_cache_stats`（各表占用）, `clear_cache`（按策略清理）, `get_cache_settings` / `set_cache_settings`

**Step 3:** 前端 Settings 页面：缓存占用统计 + 滑动条设置保留天数/大小

---

## 阶段 2: P1 高性能低占用

### Task 2.1: 后台同步调度优化

**目标:** 限制同时同步的账号数，避免多账号同时同步造成性能问题。

**文件:**
- 修改: `src-tauri/src/mail/mod.rs` — 增加并发控制
- 修改: `src/App.tsx` — 前端调度调整

**Step 1:** Rust 端增加 `std::sync::Semaphore` 限制并发 IMAP 连接数（默认 2）

**Step 2:** 后台同步按账号轮转而非同时全量

---

### Task 2.2: 缓存清理策略

**目标:** 按账号/文件夹限制本地保存天数，自动清理过期数据。

**文件:**
- 修改: `src-tauri/src/mail/mod.rs` — 同步时按策略清理
- 修改: `src-tauri/src/db/mod.rs` — 清理函数

**Step 1:** 实现 `purge_old_messages(db, retention_days)` — 删除超过保留期的邮件正文和附件

**Step 2:** 同步完成后自动触发清理

---

## 阶段 3: P2 Thunderbird 替代体验

### Task 3.1: 邮件规则/过滤器

**目标:** 按发件人、主题、关键词自动打标签/移动/标记。

**文件:**
- 新建: `src/components/FilterRules.tsx`
- 修改: `src/components/SettingsModal.tsx` — 增加"规则"Tab
- 新建: `src-tauri/src/filter.rs` — 规则引擎
- 修改: `src-tauri/src/db/mod.rs` — filter_rules 表
- 修改: `src/types/mail.ts` — FilterRule 类型
- 修改: `src/api/mail.ts` — filter API 函数

**Step 1:** 数据库：`filter_rules` 表（id, name, field, operator, value, action_type, action_value, enabled, order）

**Step 2:** Rust filter 引擎：`apply_filters(db, account_id, message_id)` — 检查所有规则并执行

**Step 3:** IMAP 同步后对新邮件自动执行过滤器

**Step 4:** 前端 FilterRules 组件：CRUD 界面

**Step 5:** 手动"应用到已有邮件"按钮

---

### Task 3.2: 统一收件箱

**目标:** "全部邮件"视图跨账号合并显示所有收件箱邮件。

**文件:**
- 修改: `src-tauri/src/commands/mod.rs` — list_messages 支持 account_id="all"
- 修改: `src/components/MailScreen.tsx` — 统一收件箱视图

**Step 1:** `list_messages` 当 account_id="all" 或 folder="INBOX" 时跨账号查询

**Step 2:** 结果合并排序，每条消息显示所属账号标签

**Step 3:** 统一收件箱搜索

---

### Task 3.3: 会话视图

**目标:** 按 Message-ID / In-Reply-To / References 建立邮件会话线程。

**文件:**
- 新建: `src/components/ConversationView.tsx`
- 修改: `src-tauri/src/db/mod.rs` — 会话索引
- 修改: `src-tauri/src/models.rs` — Conversation 类型
- 修改: `src/types/mail.ts` — Conversation 类型
- 修改: `src/api/mail.ts` — conversation API

**Step 1:** 数据库增加 `message_references` 表（message_id, in_reply_to, references）用于会话构建

**Step 2:** IMAP 同步时解析 Message-ID / In-Reply-To / References 头

**Step 3:** Rust command: `get_conversation(message_id)` 返回完整线程

**Step 4:** ConversationView 组件：树形/缩进显示会话

**Step 5:** 邮件列表切换会话/平面视图

---

### Task 3.4: 快捷键系统

**目标:** 常用操作支持键盘快捷键。

**文件:**
- 新建: `src/hooks/useKeyboardShortcuts.ts`
- 修改: `src/components/MailScreen.tsx` — 集成快捷键
- 修改: `src/App.tsx` — 全局快捷键

**Step 1:** 定义快捷键映射（Ctrl+N 新邮件, Ctrl+R 回复, Ctrl+Shift+R 全部回复, Ctrl+F 转发, Delete 归档, Ctrl+Shift+Delete 永久删除, J/K 上下导航, Ctrl+Enter 发送, Esc 关闭）

**Step 2:** useKeyboardShortcuts hook 实现

**Step 3:** 在设置中显示快捷键列表

---

### Task 3.5: 通知优化

**目标:** 新邮件通知可点开对应邮件。

**文件:**
- 修改: `src-tauri/src/commands/mod.rs` — 通知携带 message_id
- 修改: `src/App.tsx` — 处理通知点击事件

**Step 1:** Rust 通知增加 data 字段携带 message_id

**Step 2:** 前端监听通知点击事件，跳转到对应邮件

---

### Task 3.6: 导入迁移

**目标:** 支持从 mbox/eml 文件导入邮件。

**文件:**
- 新建: `src-tauri/src/import.rs` — mbox/eml 解析
- 修改: `src-tauri/Cargo.toml` — hazmat 或手动解析
- 修改: `src/components/SettingsModal.tsx` — "导入"Tab
- 修改: `src/api/mail.ts` — import API

**Step 1:** Rust mbox 解析器（按 "From " 分隔）

**Step 2:** Rust eml 解析器（单个 .eml 文件）

**Step 3:** Tauri command: `import_mbox(account_id, folder_path, file_path)` — 读取并存入消息表

**Step 4:** 前端导入界面：选择文件 → 选择目标文件夹 → 进度条 → 完成

---

## 阶段 4: 本地通讯录

### Task 4.1: 通讯录数据库和后端

**目标:** 建立联系人存储和管理 API。

**文件:**
- 新建: `src-tauri/src/contact.rs` — 通讯录模块
- 修改: `src-tauri/src/db/mod.rs` — contacts 表
- 修改: `src-tauri/src/models.rs` — Contact 模型
- 修改: `src-tauri/src/app/mod.rs` — 注册 commands

**Step 1:** 数据库：`contacts` 表（id, name, email, phone, notes, avatar_url, created_at, updated_at）

**Step 2:** Rust models: Contact, CreateContactInput, UpdateContactInput

**Step 3:** Rust commands:
- `list_contacts(search?)` — 搜索联系人
- `create_contact(input)` — 新建
- `update_contact(id, input)` — 更新
- `delete_contact(id)` — 删除
- `import_contacts_from_mail()` — 从已有邮件中提取发件人

---

### Task 4.2: 通讯录前端界面

**文件:**
- 新建: `src/components/ContactScreen.tsx`
- 修改: `src/App.tsx` — 路由集成
- 修改: `src/components/Sidebar.tsx` — 通讯录入口
- 新建: `src/api/contact.ts`
- 修改: `src/types/mail.ts` — Contact 类型

**Step 1:** 前端类型和 API

**Step 2:** ContactScreen 组件：搜索栏 + 联系人列表 + 详情面板（编辑/删除）

**Step 3:** 侧边栏增加"通讯录"入口

---

### Task 4.3: 写信时联系人自动补全

**文件:**
- 修改: `src/components/MailScreen.tsx` — 收件人输入框增加自动补全

**Step 1:** 在 toInput 输入时查询通讯录

**Step 2:** 下拉建议列表（姓名 + 邮箱）

**Step 3:** 选择后填充到收件人列表

---

## 阶段 5: Bing 翻译邮件正文

### Task 5.1: 后端翻译模块

**目标:** 通过 Microsoft Translator API (Bing) 翻译邮件正文。

**文件:**
- 新建: `src-tauri/src/translate.rs` — 翻译模块
- 修改: `src-tauri/Cargo.toml` — reqwest 已存在
- 修改: `src-tauri/src/models.rs` — TranslateInput
- 修改: `src-tauri/src/app/mod.rs` — 注册 command

**Step 1:** Microsoft Translator Text API (免费层 200 万字符/月)
- Endpoint: `https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to=zh-Hans`
- Auth: `Ocp-Apim-Subscription-Key` header
- 配置: `WOXMAIL_AZURE_TRANSLATOR_KEY` 环境变量 或设置页面输入

**Step 2:** Rust command: `translate_text(text, to_lang)` — 调用 Bing API

**Step 3:** 结果缓存到 SQLite（`translation_cache` 表：source_hash, source_lang, target_lang, translated_text）

---

### Task 5.2: 前端翻译按钮和显示

**文件:**
- 修改: `src/components/MailScreen.tsx` — 阅读区增加翻译按钮
- 新建: `src/components/TranslatePanel.tsx`
- 修改: `src/api/mail.ts` — translate API

**Step 1:** 邮件阅读工具栏增加"翻译"按钮

**Step 2:** 点击后调用后端翻译，显示在原文下方或侧面板

**Step 3:** 支持"显示原文"切换

**Step 4:** 设置页面配置目标语言（默认简体中文 zh-Hans）

---

## 阶段 6: 取消 Tauri 默认右键菜单

### Task 6.1: 禁用 WebView 默认右键菜单

**目标:** 完全禁用 Tauri WebView 的默认右键菜单。

**文件:**
- 修改: `src/main.tsx` — 添加全局 contextmenu 事件拦截
- 修改: `src-tauri/tauri.conf.json` — 如有相关配置

**Step 1:** 在 `main.tsx` 中：
```typescript
document.addEventListener("contextmenu", (event) => {
  event.preventDefault()
})
```

**Step 2:** 或者创建 `src/utils/contextMenu.ts` 统一管理：
```typescript
export function disableDefaultContextMenu() {
  window.addEventListener("contextmenu", (e) => e.preventDefault())
}
```

**Step 3:** 在 App 初始化时调用

**Step 4:** 对于需要的自定义右键菜单（邮件列表右键），手动实现并调用 `event.stopPropagation()`

---

## 实施顺序

按照用户要求的顺序：
1. Task 1.1 → 1.2 → 1.3 (P0 可靠性)
2. Task 2.1 → 2.2 (P1 性能)
3. Task 3.1 → 3.2 → 3.3 → 3.4 → 3.5 → 3.6 (P2 体验)
4. Task 4.1 → 4.2 → 4.3 (通讯录)
5. Task 5.1 → 5.2 (Bing 翻译)
6. Task 6.1 (右键菜单)

---

## 验证方式

- 每个 Task 完成后用 `cargo build` 验证编译
- 前端用 `npm run build` 验证 TypeScript 类型检查
- 功能验证通过 `npm run tauri dev` 手动测试
