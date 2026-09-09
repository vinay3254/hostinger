import React, { useState } from 'react';

interface LogViewerProps {
  logs: string;
  isStreaming?: boolean;
  isReconnecting?: boolean;
}

export const LogViewer: React.FC<LogViewerProps> = ({
  logs,
  isStreaming = false,
  isReconnecting = false,
}) => {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    if (typeof navigator !== 'undefined') {
      navigator.clipboard.writeText(logs);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const lines = logs ? logs.split('\n') : [];

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
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          padding: '8px 16px',
          backgroundColor: '#1f2937',
          borderBottom: '1px solid #374151',
          fontSize: '12px',
          color: '#9ca3af',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span>Build & Runtime Output ({lines.length} lines)</span>
          {isReconnecting ? (
            <span
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
            </span>
          ) : isStreaming ? (
            <span
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
        </div>
        <button
          onClick={handleCopy}
          style={{
            background: 'transparent',
            border: '1px solid #4b5563',
            color: '#e5e7eb',
            borderRadius: '4px',
            padding: '2px 8px',
            cursor: 'pointer',
            fontSize: '11px',
          }}
        >
          {copied ? 'Copied!' : 'Copy Logs'}
        </button>
      </div>

      <pre
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
          {lines.length > 0 ? (
            lines.map((line, idx) => (
              <div key={idx} style={{ display: 'flex', gap: '12px' }}>
                <span style={{ color: '#4b5563', userSelect: 'none', width: '32px', textAlign: 'right' }}>
                  {idx + 1}
                </span>
                <span>{line}</span>
              </div>
            ))
          ) : (
            <span style={{ color: '#6b7280' }}>No log output yet.</span>
          )}
        </code>
      </pre>
    </div>
  );
};
