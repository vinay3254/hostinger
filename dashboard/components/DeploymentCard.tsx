import React from 'react';
import Link from 'next/link';
import { Deployment } from '../lib/api';
import { StatusBadge } from './StatusBadge';

interface DeploymentCardProps {
  deployment: Deployment;
  projectId?: string;
}

export const DeploymentCard: React.FC<DeploymentCardProps> = ({ deployment, projectId }) => {
  const pId = projectId || deployment.project_id;
  const linkHref = `/projects/${pId}/deployments/${deployment.id}`;

  return (
    <div
      style={{
        border: '1px solid #e5e7eb',
        borderRadius: '8px',
        padding: '16px',
        backgroundColor: '#ffffff',
        display: 'flex',
        flexDirection: 'column',
        gap: '8px',
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
        }}
      >
        <Link
          href={linkHref}
          style={{
            fontWeight: 600,
            fontSize: '14px',
            color: '#111827',
            textDecoration: 'none',
            fontFamily: 'monospace',
          }}
        >
          {deployment.id}
        </Link>
        <StatusBadge status={deployment.status} />
      </div>

      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          fontSize: '13px',
          color: '#6b7280',
        }}
      >
        {deployment.url ? (
          <a
            href={deployment.url}
            target="_blank"
            rel="noopener noreferrer"
            style={{
              color: '#2563eb',
              textDecoration: 'none',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              maxWidth: '280px',
            }}
          >
            {deployment.url}
          </a>
        ) : (
          <span>No URL assigned</span>
        )}
        <time dateTime={deployment.created_at}>
          {new Date(deployment.created_at).toLocaleString()}
        </time>
      </div>
    </div>
  );
};
