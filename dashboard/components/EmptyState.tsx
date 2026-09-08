import React from 'react';
import Link from 'next/link';

interface EmptyStateProps {
  title: string;
  description: string;
  actionLabel?: string;
  actionHref?: string;
  onAction?: () => void;
}

export const EmptyState: React.FC<EmptyStateProps> = ({
  title,
  description,
  actionLabel,
  actionHref,
  onAction,
}) => {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '48px 24px',
        textAlign: 'center',
        border: '1px dashed #d1d5db',
        borderRadius: '12px',
        backgroundColor: '#f9fafb',
      }}
    >
      <h3 style={{ margin: '0 0 8px 0', fontSize: '18px', fontWeight: 600, color: '#111827' }}>
        {title}
      </h3>
      <p style={{ margin: '0 0 20px 0', fontSize: '14px', color: '#6b7280', maxWidth: '400px' }}>
        {description}
      </p>
      {actionLabel && actionHref && (
        <Link
          href={actionHref}
          style={{
            padding: '8px 16px',
            backgroundColor: '#111827',
            color: '#ffffff',
            borderRadius: '6px',
            fontSize: '14px',
            fontWeight: 500,
            textDecoration: 'none',
          }}
        >
          {actionLabel}
        </Link>
      )}
      {actionLabel && !actionHref && onAction && (
        <button
          onClick={onAction}
          style={{
            padding: '8px 16px',
            backgroundColor: '#111827',
            color: '#ffffff',
            borderRadius: '6px',
            fontSize: '14px',
            fontWeight: 500,
            border: 'none',
            cursor: 'pointer',
          }}
        >
          {actionLabel}
        </button>
      )}
    </div>
  );
};
