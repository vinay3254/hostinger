'use client';

import React, { useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api, SourceEvent } from '../../../../lib/api';
import { AppShell } from '../../../../components/AppShell';
import { EmptyState } from '../../../../components/EmptyState';
import { StatusBadge } from '../../../../components/StatusBadge';

export default function ProjectPreviewsPage() {
  const params = useParams();
  const projectId = params.projectId as string;
  const [activeTab, setActiveTab] = useState<'previews' | 'events'>('previews');

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const { data: previews = [], isLoading: previewsLoading } = useQuery({
    queryKey: ['previews', projectId],
    queryFn: () => api.listProjectPreviews(projectId),
    enabled: Boolean(projectId),
    refetchInterval: 3000,
  });

  const { data: events = [], isLoading: eventsLoading } = useQuery({
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
        <div style={{ marginBottom: '24px' }}>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Pull Request Previews
          </h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Isolated preview deployments and verified git events for {project?.name || 'this project'}.
          </p>
        </div>

        {/* Tab Navigation */}
        <div
          style={{
            display: 'flex',
            gap: '8px',
            borderBottom: '1px solid #e5e7eb',
            marginBottom: '20px',
          }}
        >
          <button
            type="button"
            onClick={() => setActiveTab('previews')}
            style={{
              padding: '8px 16px',
              fontSize: '14px',
              fontWeight: 600,
              color: activeTab === 'previews' ? '#2563eb' : '#6b7280',
              borderBottom: activeTab === 'previews' ? '2px solid #2563eb' : '2px solid transparent',
              background: 'none',
              borderTop: 'none',
              borderLeft: 'none',
              borderRight: 'none',
              cursor: 'pointer',
            }}
          >
            Preview Environments ({previews.length})
          </button>
          <button
            type="button"
            onClick={() => setActiveTab('events')}
            style={{
              padding: '8px 16px',
              fontSize: '14px',
              fontWeight: 600,
              color: activeTab === 'events' ? '#2563eb' : '#6b7280',
              borderBottom: activeTab === 'events' ? '2px solid #2563eb' : '2px solid transparent',
              background: 'none',
              borderTop: 'none',
              borderLeft: 'none',
              borderRight: 'none',
              cursor: 'pointer',
            }}
          >
            Source Events ({events.length})
          </button>
        </div>

        {activeTab === 'previews' ? (
          previewsLoading ? (
            <div style={{ padding: '40px', textAlign: 'center', color: '#6b7280' }}>
              Loading preview deployments...
            </div>
          ) : previews.length === 0 ? (
            <EmptyState
              title="No preview deployments"
              description="Pull request webhooks will automatically trigger isolated preview environments with unique hostnames."
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
                    <th style={{ padding: '12px 16px', fontWeight: 600 }}>Pull Request</th>
                    <th style={{ padding: '12px 16px', fontWeight: 600 }}>Status</th>
                    <th style={{ padding: '12px 16px', fontWeight: 600 }}>Commit</th>
                    <th style={{ padding: '12px 16px', fontWeight: 600 }}>Hostname / URL</th>
                    <th style={{ padding: '12px 16px', fontWeight: 600 }}>Logs</th>
                    <th style={{ padding: '12px 16px', fontWeight: 600 }}>Updated</th>
                    <th style={{ padding: '12px 16px', fontWeight: 600, textAlign: 'right' }}>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {previews.map((preview) => (
                    <tr key={preview.id} style={{ borderBottom: '1px solid #f3f4f6' }}>
                      <td style={{ padding: '14px 16px' }}>
                        <div>
                          <strong style={{ color: '#111827' }}>PR #{preview.pr_number}</strong>
                          <span style={{ marginLeft: '6px', fontSize: '12px', color: '#6b7280' }}>
                            ({preview.head_branch} &rarr; {preview.base_branch})
                          </span>
                        </div>
                        <div style={{ fontSize: '11px', color: '#9ca3af', textTransform: 'capitalize', marginTop: '2px' }}>
                          Provider: {preview.provider}
                        </div>
                      </td>
                      <td style={{ padding: '14px 16px' }}>
                        <StatusBadge status={preview.status} />
                      </td>
                      <td style={{ padding: '14px 16px', fontFamily: 'monospace', color: '#4b5563' }}>
                        {preview.head_sha.slice(0, 7)}
                      </td>
                      <td style={{ padding: '14px 16px' }}>
                        {preview.status === 'closed' ? (
                          <span style={{ color: '#9ca3af', fontSize: '12px', fontStyle: 'italic' }}>
                            Closed / Expired
                          </span>
                        ) : preview.status === 'ready' ? (
                          <a
                            href={`http://${preview.hostname}`}
                            target="_blank"
                            rel="noopener noreferrer"
                            style={{ color: '#2563eb', fontWeight: 500, textDecoration: 'none' }}
                          >
                            {preview.hostname}
                          </a>
                        ) : (
                          <span style={{ color: '#6b7280', fontFamily: 'monospace', fontSize: '12px' }}>
                            {preview.hostname}
                          </span>
                        )}
                      </td>
                      <td style={{ padding: '14px 16px' }}>
                        {preview.deployment_id ? (
                          <Link
                            href={`/projects/${projectId}/deployments/${preview.deployment_id}`}
                            style={{ color: '#2563eb', fontSize: '12px', textDecoration: 'none' }}
                          >
                            View Logs &rarr;
                          </Link>
                        ) : (
                          <span style={{ color: '#9ca3af', fontSize: '12px' }}>-</span>
                        )}
                      </td>
                      <td style={{ padding: '14px 16px', color: '#6b7280', fontSize: '12px' }}>
                        {new Date(preview.updated_at).toLocaleString()}
                      </td>
                      <td style={{ padding: '14px 16px', textAlign: 'right' }}>
                        <Link
                          href={`/projects/${projectId}/previews/${preview.id}`}
                          style={{
                            padding: '6px 12px',
                            backgroundColor: '#f3f4f6',
                            color: '#374151',
                            borderRadius: '6px',
                            fontSize: '12px',
                            fontWeight: 500,
                            textDecoration: 'none',
                            border: '1px solid #d1d5db',
                          }}
                        >
                          Details
                        </Link>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        ) : eventsLoading ? (
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
