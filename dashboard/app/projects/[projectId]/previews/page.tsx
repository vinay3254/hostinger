'use client';

import React from 'react';
import { useParams } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api, SourceEvent } from '../../../../lib/api';
import { AppShell } from '../../../../components/AppShell';
import { EmptyState } from '../../../../components/EmptyState';

export default function ProjectPreviewsPage() {
  const params = useParams();
  const projectId = params.projectId as string;

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const { data: events = [], isLoading } = useQuery({
    queryKey: ['source-events', projectId],
    queryFn: () => api.listProjectSourceEvents(projectId),
    enabled: Boolean(projectId),
  });

  const getEventBadge = (kind: SourceEvent['kind']) => {
    switch (kind) {
      case 'push':
        return (
          <span
            style={{
              padding: '2px 8px',
              borderRadius: '4px',
              fontSize: '12px',
              fontWeight: 600,
              backgroundColor: '#ecfdf5',
              color: '#065f46',
            }}
          >
            Push
          </span>
        );
      case 'pull_request_opened':
        return (
          <span
            style={{
              padding: '2px 8px',
              borderRadius: '4px',
              fontSize: '12px',
              fontWeight: 600,
              backgroundColor: '#eff6ff',
              color: '#1d4ed8',
            }}
          >
            PR Opened
          </span>
        );
      case 'pull_request_updated':
        return (
          <span
            style={{
              padding: '2px 8px',
              borderRadius: '4px',
              fontSize: '12px',
              fontWeight: 600,
              backgroundColor: '#fef3c7',
              color: '#92400e',
            }}
          >
            PR Updated
          </span>
        );
      case 'pull_request_closed':
        return (
          <span
            style={{
              padding: '2px 8px',
              borderRadius: '4px',
              fontSize: '12px',
              fontWeight: 600,
              backgroundColor: '#f3f4f6',
              color: '#4b5563',
            }}
          >
            PR Closed
          </span>
        );
    }
  };

  return (
    <AppShell currentProjectId={projectId}>
      <div style={{ maxWidth: '1000px', margin: '0 auto' }}>
        <div style={{ marginBottom: '28px' }}>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Pull Request Previews & Source Events
          </h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Verified git webhooks and pull request preview events for {project?.name || 'this project'}.
          </p>
        </div>

        {isLoading ? (
          <div style={{ padding: '40px', textAlign: 'center', color: '#6b7280' }}>
            Loading source events...
          </div>
        ) : events.length === 0 ? (
          <EmptyState
            title="No source events received"
            description="Verified git events and PR previews will appear here as soon as webhooks are triggered."
            actionLabel="Configure Git Source"
            actionHref={`/projects/${projectId}/settings/source`}
          />
        ) : (
          <div
            style={{
              backgroundColor: '#ffffff',
              border: '1px solid #e5e7eb',
              borderRadius: '8px',
              overflow: 'hidden',
              boxShadow: '0 1px 3px rgba(0,0,0,0.05)',
            }}
          >
            <table style={{ width: '100%', borderCollapse: 'collapse', textAlign: 'left', fontSize: '13px' }}>
              <thead>
                <tr style={{ backgroundColor: '#f9fafb', borderBottom: '1px solid #e5e7eb', color: '#6b7280' }}>
                  <th style={{ padding: '12px 16px', fontWeight: 600 }}>Event</th>
                  <th style={{ padding: '12px 16px', fontWeight: 600 }}>Ref / Branch</th>
                  <th style={{ padding: '12px 16px', fontWeight: 600 }}>Commit SHA</th>
                  <th style={{ padding: '12px 16px', fontWeight: 600 }}>Provider</th>
                  <th style={{ padding: '12px 16px', fontWeight: 600 }}>Delivery ID</th>
                  <th style={{ padding: '12px 16px', fontWeight: 600, textAlign: 'right' }}>Received</th>
                </tr>
              </thead>
              <tbody>
                {events.map((ev) => (
                  <tr key={ev.id} style={{ borderBottom: '1px solid #f3f4f6' }}>
                    <td style={{ padding: '14px 16px' }}>{getEventBadge(ev.kind)}</td>
                    <td style={{ padding: '14px 16px' }}>
                      {ev.pull_request ? (
                        <span>
                          <strong>PR #{ev.pull_request.number}</strong> ({ev.pull_request.base_branch})
                        </span>
                      ) : (
                        <span style={{ fontFamily: 'monospace' }}>{ev.branch || 'unknown'}</span>
                      )}
                    </td>
                    <td style={{ padding: '14px 16px', fontFamily: 'monospace', color: '#4b5563' }}>
                      {ev.commit_sha.slice(0, 8)}
                    </td>
                    <td style={{ padding: '14px 16px', textTransform: 'capitalize' }}>
                      {ev.provider}
                    </td>
                    <td style={{ padding: '14px 16px', fontFamily: 'monospace', color: '#6b7280', fontSize: '12px' }}>
                      {ev.delivery_id.slice(0, 12)}...
                    </td>
                    <td style={{ padding: '14px 16px', textAlign: 'right', color: '#6b7280' }}>
                      {new Date(ev.created_at).toLocaleString()}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </AppShell>
  );
}
