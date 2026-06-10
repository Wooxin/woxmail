import { useState, useMemo, useRef, useEffect } from "react"
import DOMPurify from "dompurify"
import { openExternalUrl } from "../api/open"

type Props = {
  html: string
  text: string
  dark: boolean
}

const ALLOWED_TAGS = [
  "h1", "h2", "h3", "h4", "h5", "h6",
  "p", "div", "span", "br", "hr",
  "ul", "ol", "li",
  "strong", "b", "em", "i", "u", "s", "del",
  "a", "img",
  "table", "thead", "tbody", "tr", "th", "td",
  "blockquote", "pre", "code",
  "sub", "sup", "small",
]

const ALLOWED_ATTR = [
  "href", "title", "target",
  "src", "alt", "width", "height",
  "style", "class", "id",
  "align", "valign",
  "colspan", "rowspan",
  "cellpadding", "cellspacing", "border",
]

function HtmlRenderer({ html, text, dark }: Props) {
  const [remoteContentLoaded, setRemoteContentLoaded] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)

  // Sanitize HTML - first pass removes remote images
  const sanitizedHtml = useMemo(() => {
    if (!html) return null

    const config = {
      ALLOWED_TAGS,
      ALLOWED_ATTR,
      ALLOW_DATA_ATTR: false,
      ADD_TAGS: [] as string[],
      FORBID_TAGS: ["script", "style", "iframe", "object", "embed", "form", "input", "button"],
      FORBID_ATTR: ["onclick", "onload", "onerror", "onmouseover", "onfocus", "onblur"],
    }

    let cleaned = DOMPurify.sanitize(html, config)

    // Remove remote images unless explicitly loaded
    if (!remoteContentLoaded) {
      cleaned = cleaned.replace(
        /<img\s[^>]*src\s*=\s*["']https?:\/\/[^"']+["'][^>]*>/gi,
        '<div class="remote-img-placeholder" style="display:inline-block;padding:8px 16px;margin:4px 0;border-radius:8px;border:1px dashed;font-size:13px;cursor:default;">🖼️ 远程图片已阻止 (点击下方按钮加载)</div>',
      )
      // Also remove background-image URLs that are remote
      cleaned = cleaned.replace(
        /url\(\s*["']?https?:\/\/[^)"']+["']?\s*\)/gi,
        "none",
      )
    }

    return cleaned
  }, [html, remoteContentLoaded])

  // Intercept all link clicks → open in system browser
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      const anchor = target.closest("a")
      if (anchor && (anchor as HTMLAnchorElement).href?.startsWith("http")) {
        e.preventDefault()
        openExternalUrl((anchor as HTMLAnchorElement).href)
      }
    }
    el.addEventListener("click", handler)
    return () => el.removeEventListener("click", handler)
  }, [sanitizedHtml])

  const textColor = dark ? "#e4e4e7" : "#18181b"
  const linkColor = dark ? "#60a5fa" : "#2563eb"
  const quoteColor = dark ? "#52525b" : "#a1a1aa"
  const borderColor = dark ? "#3f3f46" : "#d4d4d8"
  const codeBg = dark ? "#27272a" : "#f4f4f5"
  const tableBorder = dark ? "#3f3f46" : "#d4d4d8"

  if (!sanitizedHtml) {
    // Plain text fallback
    return (
      <div
        style={{ color: textColor, lineHeight: 1.7, whiteSpace: "pre-wrap", wordBreak: "break-word" }}
      >
        {text}
      </div>
    )
  }

  return (
    <div className="mail-body">
      {/* Show toggle between HTML and plain text */}
      {!remoteContentLoaded && html && (
        <div
          style={{
            marginBottom: 12,
            padding: "8px 14px",
            borderRadius: 10,
            backgroundColor: dark ? "rgba(59,130,246,0.1)" : "rgba(59,130,246,0.08)",
            border: `1px solid ${dark ? "rgba(59,130,246,0.3)" : "rgba(59,130,246,0.2)"}`,
            fontSize: 13,
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          <span style={{ color: dark ? "#93c5fd" : "#3b82f6" }}>📧</span>
          <span style={{ color: dark ? "#e4e4e7" : "#18181b", flex: 1 }}>
            为了保护隐私，远程图片已自动阻止。
          </span>
          <button
            onClick={() => setRemoteContentLoaded(true)}
            style={{
              padding: "4px 12px",
              borderRadius: 6,
              border: "none",
              backgroundColor: dark ? "rgba(59,130,246,0.3)" : "rgba(59,130,246,0.15)",
              color: dark ? "#bfdbfe" : "#1d4ed8",
              cursor: "pointer",
              fontSize: 13,
              fontWeight: 500,
            }}
          >
            加载远程内容
          </button>
        </div>
      )}

      <div
        ref={containerRef}
        dangerouslySetInnerHTML={{ __html: sanitizedHtml as unknown as string }}
        className="html-body"
      />
      <style>{`
        .html-body {
          color: ${textColor};
          line-height: 1.7;
          word-break: break-word;
        }
        .html-body a {
          color: ${linkColor};
          text-decoration: underline;
        }
        .html-body blockquote {
          border-left: 3px solid ${quoteColor};
          margin: 8px 0;
          padding: 4px 12px;
          color: ${quoteColor};
        }
        .html-body pre, .html-body code {
          background: ${codeBg};
          border-radius: 6px;
          padding: 2px 6px;
          font-size: 0.9em;
          font-family: ui-monospace, monospace;
        }
        .html-body pre {
          padding: 12px;
          overflow-x: auto;
        }
        .html-body pre code {
          background: none;
          padding: 0;
        }
        .html-body table {
          border-collapse: collapse;
          width: 100%;
          margin: 8px 0;
        }
        .html-body th, .html-body td {
          border: 1px solid ${tableBorder};
          padding: 6px 10px;
          text-align: left;
        }
        .html-body th {
          background: ${dark ? "#27272a" : "#f4f4f5"};
          font-weight: 600;
        }
        .html-body img {
          max-width: 100%;
          height: auto;
          border-radius: 6px;
        }
        .html-body ul, .html-body ol {
          padding-left: 24px;
        }
        .remote-img-placeholder {
          color: ${dark ? "#a1a1aa" : "#71717a"};
          background: ${dark ? "#27272a" : "#f4f4f5"};
          border-color: ${borderColor};
        }
      `}</style>
    </div>
  )
}

export default HtmlRenderer
