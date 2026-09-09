import React from 'react';
import { DeploymentStatus } from '../lib/api';

interface StatusBadgeProps {
  status: DeploymentStatus;
  className?: string;
}

export const StatusBadge: React.FC<StatusBadgeProps> = ({ status, className = '' }) => {
  const getColors = () => {
    switch (status) {
      case 'running':
        return {
          bg: '#ecfdf5',
          text: '#065f46',
          border: '#a7f3d0',
          dot: '#10b981',
        };
      case 'building':
        return {
          bg: '#eff6ff',
          text: '#1e40af',
          border: '#bfdbfe',
          dot: '#3b82f6',
        };
      case 'failed':
        return {
          bg: '#fef2f2',
          text: '#991b1b',
          border: '#fecaca',
          dot: '#ef4444',
        };
      case 'stopped':
        return {
          bg: '#f3f4f6',
          text: '#374151',
          border: '#e5e7eb',
          dot: '#9ca3af',
        };
      case 'queued':
        return {
          bg: '#f5f3ff',
          text: '#5b21b6',
          border: '#ddd6fe',
          dot: '#8b5cf6',
        };
      case 'retrying':
        return {
          bg: '#fff7ed',
          text: '#9a3412',
          border: '#fed7aa',
          dot: '#f97316',
        };
      case 'cancelled':
        return {
          bg: '#f3f4f6',
          text: '#4b5563',
          border: '#d1d5db',
          dot: '#6b7280',
        };
      case 'pending':
      default:
        return {
          bg: '#fffbeb',
          text: '#92400e',
          border: '#fde68a',
          dot: '#f59e0b',
        };
    }
  };

  const colors = getColors();

  return (
    <span
      className={`badge badge-${status} ${className}`}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: '6px',
        padding: '2px 8px',
        borderRadius: '9999px',
        fontSize: '12px',
        fontWeight: 500,
        textTransform: 'capitalize',
        backgroundColor: colors.bg,
        color: colors.text,
        border: `1px solid ${colors.border}`,
      }}
    >
      <span
        style={{
          width: '6px',
          height: '6px',
          borderRadius: '50%',
          backgroundColor: colors.dot,
        }}
      />
      {status}
    </span>
  );
};
