import { useEffect, useState } from "react"
import { AlertTriangle, Clock, RefreshCw, Trash2, Send, Loader2 } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { OutboxJob } from "../types/mail"
import { listOutboxJobs, retryOutboxJob, cancelOutboxJob, processOutbox } from "../api/mail"

type Props = {
  dark: boolean
}

function formatTs(ts: number) {
  const d = new Date(ts * 1000)
  return d.toLocaleString()
}

function OutboxScreen({ dark }: Props) {
  useTranslation()
  const [jobs, setJobs] = useState<OutboxJob[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [actioning, setActioning] = useState<Record<string, string>>({})

  const loadJobs = async () => {
    setLoading(true)
    setError(null)
    try {
      setJobs(await listOutboxJobs())
    } catch (e) {
      setError(getErrorMessage(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void loadJobs()
    const id = window.setInterval(() => void loadJobs(), 15_000)
    return () => window.clearInterval(id)
  }, [])

  const handleRetry = async (jobId: string) => {
    setActioning((prev) => ({ ...prev, [jobId]: "retrying" }))
    try {
      await retryOutboxJob(jobId)
      await processOutbox()
      await loadJobs()
    } catch (e) {
      setError(getErrorMessage(e))
    } finally {
      setActioning((prev) => {
        const next = { ...prev }
        delete next[jobId]
        return next
      })
    }
  }

  const handleRetryAll = async () => {
    setActioning((prev) => ({ ...prev, _all: "retrying" }))
    try {
      await processOutbox()
      await loadJobs()
    } catch (e) {
      setError(getErrorMessage(e))
    } finally {
      setActioning((prev) => {
        const next = { ...prev }
        delete next._all
        return next
      })
    }
  }

  const handleCancel = async (jobId: string) => {
    if (!window.confirm("确定要取消发送这封邮件？取消后不可恢复。")) return
    setActioning((prev) => ({ ...prev, [jobId]: "cancelling" }))
    try {
      await cancelOutboxJob(jobId)
      await loadJobs()
    } catch (e) {
      setError(getErrorMessage(e))
    } finally {
      setActioning((prev) => {
        const next = { ...prev }
        delete next[jobId]
        return next
      })
    }
  }

  const pendingCount = jobs.filter((j) => j.status === "pending").length
  const failedCount = jobs.filter((j) => j.status === "failed").length

  return (
    <div className="flex h-full flex-col">
      <div className={`flex shrink-0 items-center justify-between border-b p-5 ${dark ? "border-white/10" : "border-black/10"}`}>
        <div>
          <h2 className="text-lg font-semibold">发件箱</h2>
          <div className={`mt-1 text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
            {pendingCount > 0 && (
              <span className="inline-flex items-center gap-1">
                <Clock size={14} />
                {pendingCount} 封待发送
              </span>
            )}
            {failedCount > 0 && (
              <span className="ml-3 inline-flex items-center gap-1 text-red-400">
                <AlertTriangle size={14} />
                {failedCount} 封发送失败
              </span>
            )}
            {jobs.length === 0 && "暂无待发邮件"}
          </div>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => void loadJobs()}
            className={`rounded-xl p-2 transition ${dark ? "hover:bg-white/10" : "hover:bg-black/10"}`}
            title="刷新"
          >
            <RefreshCw size={18} className={loading ? "animate-spin" : ""} />
          </button>
          {jobs.length > 0 && (
            <button
              onClick={() => void handleRetryAll()}
              disabled={!!actioning._all}
              className="flex items-center gap-2 rounded-xl bg-blue-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-600 disabled:opacity-50"
            >
              {actioning._all === "retrying" ? <Loader2 size={16} className="animate-spin" /> : <Send size={16} />}
              全部重试
            </button>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {error && (
          <div className="m-4 rounded-2xl border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-300">
            {error}
          </div>
        )}

        {loading && jobs.length === 0 ? (
          <div className={`flex items-center justify-center p-12 ${dark ? "text-zinc-500" : "text-zinc-600"}`}>
            <Loader2 size={24} className="animate-spin" />
            <span className="ml-3">加载中…</span>
          </div>
        ) : jobs.length === 0 ? (
          <div className={`flex flex-col items-center justify-center p-12 ${dark ? "text-zinc-500" : "text-zinc-600"}`}>
            <Send size={48} className="mb-4 opacity-30" />
            <div className="text-lg">发件箱为空</div>
            <div className="mt-1 text-sm">所有邮件已成功发送</div>
          </div>
        ) : (
          <div className="divide-y">
            {jobs.map((job) => (
              <div
                key={job.id}
                className={`p-4 transition ${dark ? "divide-white/5 hover:bg-white/5" : "divide-black/5 hover:bg-black/5"}`}
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <StatusBadge status={job.status} dark={dark} />
                      <span className="font-medium truncate">{job.subject || "(无主题)"}</span>
                    </div>
                    <div className={`mt-1 truncate text-sm ${dark ? "text-zinc-400" : "text-zinc-600"}`}>
                      收件人: {job.to_emails.join(", ")}
                    </div>
                    <div className={`mt-1 flex items-center gap-3 text-xs ${dark ? "text-zinc-500" : "text-zinc-500"}`}>
                      <span>创建于 {formatTs(job.created_at)}</span>
                      {job.attempts > 0 && <span>已尝试 {job.attempts} 次</span>}
                      {job.next_attempt_at > 0 && job.status === "failed" && (
                        <span>下次重试: {formatTs(job.next_attempt_at)}</span>
                      )}
                    </div>
                    {job.last_error && (
                      <div className="mt-2 rounded-xl border border-red-500/20 bg-red-500/5 p-3 text-xs text-red-400">
                        {job.last_error}
                      </div>
                    )}
                  </div>
                  <div className="flex shrink-0 gap-2">
                    {(job.status === "failed" || job.status === "pending") && (
                      <button
                        onClick={() => void handleRetry(job.id)}
                        disabled={!!actioning[job.id]}
                        className={`flex items-center gap-1 rounded-xl px-3 py-2 text-xs transition ${
                          dark ? "bg-blue-500/20 text-blue-300 hover:bg-blue-500/30" : "bg-blue-500/10 text-blue-600 hover:bg-blue-500/20"
                        } disabled:opacity-50`}
                      >
                        {actioning[job.id] === "retrying" ? (
                          <Loader2 size={14} className="animate-spin" />
                        ) : (
                          <Send size={14} />
                        )}
                        重试
                      </button>
                    )}
                    <button
                      onClick={() => void handleCancel(job.id)}
                      disabled={!!actioning[job.id]}
                      className={`flex items-center gap-1 rounded-xl px-3 py-2 text-xs transition ${
                        dark ? "bg-red-500/20 text-red-300 hover:bg-red-500/30" : "bg-red-500/10 text-red-600 hover:bg-red-500/20"
                      } disabled:opacity-50`}
                    >
                      {actioning[job.id] === "cancelling" ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : (
                        <Trash2 size={14} />
                      )}
                      取消
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function StatusBadge({ status, dark }: { status: string; dark: boolean }) {
  const config: Record<string, { label: string; className: string }> = {
    pending: {
      label: "待发送",
      className: dark ? "bg-yellow-500/20 text-yellow-300" : "bg-yellow-500/10 text-yellow-700",
    },
    sending: {
      label: "发送中",
      className: dark ? "bg-blue-500/20 text-blue-300" : "bg-blue-500/10 text-blue-700",
    },
    failed: {
      label: "失败",
      className: dark ? "bg-red-500/20 text-red-300" : "bg-red-500/10 text-red-700",
    },
  }
  const c = config[status] ?? { label: status, className: dark ? "bg-white/10 text-zinc-400" : "bg-black/10 text-zinc-600" }
  return (
    <span className={`inline-block rounded-lg px-2 py-0.5 text-xs font-medium ${c.className}`}>
      {c.label}
    </span>
  )
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "string") return error
  return "未知错误"
}

export default OutboxScreen
