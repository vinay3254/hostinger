import React, { useState, useRef, useEffect, useMemo } from 'react';

interface LogViewerProps {
  logs: string;
  deploymentId?: string;
  isStreaming?: boolean;
  isReconnecting?: boolean;
  onReconnect?: () => void;
}

// Client-side safety regexes to ensure secrets are never exposed in UI
const BEARER_REGEX = /bearer\s+[a-zA-Z0-9_\-\.\~]+/gi;
const AWS_REGEX = /AKIA[0-9A-Z]{16}/g;
const PRIV_KEY_REGEX = /-----BEGIN [A-Z ]+PRIVATE KEY-----[\s\S]*?-----END [A-Z ]+PRIVATE KEY-----/g;
const ASSIGN_REGEX = /\b(password|passwd|secret|token|api_key|access_token|auth_token)\s*([=:])\s*([^\s&]+)/gi;
const TOKEN_REGEX = /\b(ghp_[A-Za-z0-9]{36}|glpat-[A-Za-z0-9_-]{20,}|xox[baprs]-[A-Za-z0-9-]+)\b/g;

function maskSecrets(line: string): string {
  if (!line) return '';
  return line
    .replace(PRIV_KEY_REGEX, '[REDACTED PRIVATE KEY]')
    .replace(AWS_REGEX, '[REDACTED]')
    .replace(BEARER_REGEX, 'Bearer [REDACTED]')
    .replace(TOKEN_REGEX, '[REDACTED]')
    .replace(ASSIGN_REGEX, '$1$2[REDACTED]');
}

export const LogViewer: React.FC<LogViewerProps> = ({
  logs,
  deploymentId,
  isStreaming = false,
  isReconnecting = false,
  onReconnect,
}) => {
  const [copied, setCopied] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [follow, setFollow] = useState(true);
  const logContainerRef = useRef<HTMLPreElement | null>(null);

  const rawLines = useMemo(() => (logs ? logs.split('\n') : []), [logs]);

  // Mask sensitive credentials client-side
  const sanitizedLines = useMemo(
    () => rawLines.map((line) => maskSecrets(line)),
    [rawLines]
  );

  // Filter lines if search query is provided
  const filteredLinesWithIndex = useMemo(() => {
    if (!searchQuery.trim()) {
      return sanitizedLines.map((line, originalIndex) => ({
        line,
        originalIndex,
      }));
    }
    const query = searchQuery.toLowerCase();
    return sanitizedLines
      .map((line, originalIndex) => ({ line, originalIndex }))
      .filter(({ line }) => line.toLowerCase().includes(query));
  }, [sanitizedLines, searchQuery]);

  // Auto-scroll when follow is active
  useEffect(() => {
    if (follow && logContainerRef.current) {
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [sanitizedLines, follow]);

  const handleCopy = () => {
    if (typeof navigator !== 'undefined') {
      navigator.clipboard.writeText(sanitizedLines.join('\n'));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleDownload = () => {
    if (typeof window === 'undefined') return;
    const blob = new Blob([sanitizedLines.join('\n')], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `deployment-${deploymentId || 'logs'}.log`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  };

  return (
    <div
      style={{
        borderRadius: '8px',
        overflow: 'hidden',
        border: '1px solid #1f2937',
        backgroundColor: '#111827',
        color: '#f3f4f6',
        fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
        fontSize: '13px',
      }}
    >
      {/* Control Bar */}
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '12px',
          padding: '8px 16px',
          backgroundColor: '#1f2937',
          borderBottom: '1px solid #374151',
          fontSize: '12px',
          color: '#9ca3af',
        }}
      >
        {/* Left: Line count and streaming state */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span>
            Build & Runtime Output ({sanitizedLines.length} lines
            {searchQuery.trim() ? `, ${filteredLinesWithIndex.length} matched` : ''})
          </span>

          {isReconnecting ? (
            <span
              role="status"
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: '4px',
                color: '#f59e0b',
                backgroundColor: 'rgba(245, 158, 11, 0.1)',
                padding: '2px 6px',
                borderRadius: '4px',
                fontSize: '11px',
              }}
            >
              ● Reconnecting...
              {onReconnect && (
                <button
                  onClick={onReconnect}
                  style={{
                    background: 'transparent',
                    border: 'none',
                    color: '#f59e0b',
                    textDecoration: 'underline',
                    cursor: 'pointer',
                    fontSize: '11px',
                    padding: 0,
                    marginLeft: '4px',
                  }}
                >
                  Retry
                </button>
              )}
            </span>
          ) : isStreaming ? (
            <span
              role="status"
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: '4px',
                color: '#10b981',
                backgroundColor: 'rgba(16, 185, 129, 0.1)',
                padding: '2px 6px',
                borderRadius: '4px',
                fontSize: '11px',
              }}
            >
              ● Live streaming
            </span>
          ) : null}

          {!follow && (
            <span
              style={{
                color: '#f97316',
                backgroundColor: 'rgba(249, 115, 22, 0.1)',
                padding: '2px 6px',
                borderRadius: '4px',
                fontSize: '11px',
              }}
            >
              Scroll Paused
            </span>
          )}
        </div>

        {/* Right: Search, Pause/Follow, Copy, Download */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          {/* Search Input */}
          <input
            type="search"
            aria-label="Search logs"
            placeholder="Search logs..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            style={{
              padding: '3px 8px',
              fontSize: '12px',
              backgroundColor: '#111827',
              color: '#f3f4f6',
              border: '1px solid #4b5563',
              borderRadius: '4px',
              outline: 'none',
              width: '140px',
            }}
          />

          {/* Follow / Pause Toggle */}
          <button
            onClick={() => setFollow(!follow)}
            aria-pressed={follow}
            aria-label={follow ? 'Pause log auto-scroll' : 'Resume log auto-scroll'}
            style={{
              background: follow ? 'rgba(59, 130, 246, 0.1)' : 'transparent',
              border: follow ? '1px solid #3b82f6' : '1px solid #4b5563',
              color: follow ? '#60a5fa' : '#9ca3af',
              borderRadius: '4px',
              padding: '3px 8px',
              cursor: 'pointer',
              fontSize: '11px',
            }}
          >
            {follow ? 'Following' : 'Follow'}
          </button>

          {/* Copy Button */}
          <button
            onClick={handleCopy}
            aria-label="Copy logs to clipboard"
            style={{
              background: 'transparent',
              border: '1px solid #4b5563',
              color: '#e5e7eb',
              borderRadius: '4px',
              padding: '3px 8px',
              cursor: 'pointer',
              fontSize: '11px',
            }}
          >
            {copied ? 'Copied!' : 'Copy'}
          </button>

          {/* Download Button */}
          <button
            onClick={handleDownload}
            aria-label="Download logs as text file"
            style={{
              background: 'transparent',
              border: '1px solid #4b5563',
              color: '#e5e7eb',
              borderRadius: '4px',
              padding: '3px 8px',
              cursor: 'pointer',
              fontSize: '11px',
            }}
          >
            Download
          </button>
        </div>
      </div>

      {/* Log Output Stream */}
      <pre
        ref={logContainerRef}
        role="log"
        aria-label="Build and runtime logs"
        aria-live="polite"
        style={{
          margin: 0,
          padding: '16px',
          overflowX: 'auto',
          maxHeight: '480px',
          lineHeight: '1.5',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-all',
        }}
      >
        <code>
          {filteredLinesWithIndex.length > 0 ? (
            filteredLinesWithIndex.map(({ line, originalIndex }) => (
              <div key={originalIndex} style={{ display: 'flex', gap: '12px' }}>
                <span
                  style={{
                    color: '#4b5563',
                    userSelect: 'none',
                    width: '36px',
                    textAlign: 'right',
                    flexShrink: 0,
                  }}
                >
                  {originalIndex + 1}
                </span>
                <span style={{ flex: 1 }}>{line}</span>
              </div>
            ))
          ) : (
            <span style={{ color: '#6b7280' }}>
              {searchQuery.trim() ? 'No lines match the search filter.' : 'No log output yet.'}
            </span>
          )}
        </code>
      </pre>
    </div>
  );
};
