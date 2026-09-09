'use client';

import React, { useState } from 'react';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api, Preview } from '../../../../../lib/api';
import { AppShell } from '../../../../../components/AppShell';
import { StatusBadge } from '../../../../../components/StatusBadge';

export default function PreviewDetailPage() {
  const params = useParams();
  const router = useRouter();
  const queryClient = useQueryClient();
  const projectId = params.projectId as string;
  const previewId = params.previewId as string;

  const [actionError, setActionError] = useState<string | null>(null);
  const [promotedDeploymentId, setPromotedDeploymentId] = useState<string | null>(null);

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const { data: preview, isLoading } = useQuery({
    queryKey: ['preview', previewId],
    queryFn: () => api.getPreview(previewId),
    enabled: Boolean(previewId),
    refetchInterval: (query) => {
      const data = query.state.data;
      if (data && (data.status === 'building')) {
        return 2000;
      }
      return false;
    },
  });

  const stopMutation = useMutation({
    mutationFn: () => api.stopPreview(previewId),
    onSuccess: (updated) => {
      setActionError(null);
      queryClient.setQueryData(['preview', previewId], updated);
      queryClient.invalidateQueries({ queryKey: ['previews', projectId] });
    },
    onError: (err: any) => {
      setActionError(err.message || 'Failed to stop preview deployment');
    },
  });

  const promoteMutation = useMutation({
    mutationFn: () => api.promotePreview(previewId),
    onSuccess: (deployment) => {
      setActionError(null);
      setPromotedDeploymentId(deployment.id);
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
    },
    onError: (err: any) => {
      setActionError(err.message || 'Failed to promote preview to production');
    },
  });

  if (isLoading) {
    return (
      <AppShell currentProjectId={projectId}>
        <div style={{ maxWidth: '900px', margin: '0 auto', padding: '40px 0', textAlign: 'center', color: '#6b7280' }}>
          Loading preview details...
        </div>
      </AppShell>
    );
  }

  if (!preview) {
    return (
      <AppShell currentProjectId={projectId}>
        <div style={{ maxWidth: '900px', margin: '0 auto', padding: '40px 0', textAlign: 'center' }}>
          <h2 style={{ fontSize: '18px', fontWeight: 600, color: '#111827' }}>Preview Not Found</h2>
          <p style={{ color: '#6b7280', fontSize: '14px', marginTop: '8px' }}>
            The requested preview deployment does not exist or has been removed.
          </p>
          <Link
            href={`/projects/${projectId}/previews`}
            style={{
              display: 'inline-block',
              marginTop: '16px',
              padding: '8px 16px',
              backgroundColor: '#2563eb',
              color: '#ffffff',
              borderRadius: '6px',
              fontSize: '14px',
              textDecoration: 'none',
            }}
          >
            Back to Previews
          </Link>
        </div>
      </AppShell>
    );
  }

  const isClosed = preview.status === 'closed';

  return (
    <AppShell currentProjectId={projectId}>
      <div style={{ maxWidth: '900px', margin: '0 auto' }}>
        {/* Navigation Breadcrumb */}
        <div style={{ marginBottom: '16px', fontSize: '14px', color: '#6b7280' }}>
          <Link href={`/projects/${projectId}/previews`} style={{ color: '#2563eb', textDecoration: 'none' }}>
            &larr; Previews
          </Link>
          <span style={{ margin: '0 8px' }}>/</span>
          <span>PR #{preview.pr_number}</span>
        </div>

        {/* Header */}
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'flex-start',
            flexWrap: 'wrap',
            gap: '16px',
            marginBottom: '24px',
          }}
        >
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
              <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
                Preview: PR #{preview.pr_number}
              </h1>
              <StatusBadge status={preview.status} />
            </div>
            <p style={{ margin: '6px 0 0 0', fontSize: '14px', color: '#4b5563' }}>
              Branch <strong>{preview.head_branch}</strong> into <strong>{preview.base_branch}</strong>
            </p>
          </div>

          {/* Action Buttons */}
          <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
            {!isClosed && preview.status === 'ready' && (
              <a
                href={`http://${preview.hostname}`}
                target="_blank"
                rel="noopener noreferrer"
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#2563eb',
                  color: '#ffffff',
                  borderRadius: '6px',
                  fontSize: '13px',
                  fontWeight: 600,
                  textDecoration: 'none',
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '6px',
                }}
              >
                Open Preview ↗
              </a>
            )}

            {!isClosed && (
              <>
                <button
                  type="button"
                  onClick={() => promoteMutation.mutate()}
                  disabled={promoteMutation.isPending}
                  style={{
                    padding: '8px 16px',
                    backgroundColor: '#10b981',
                    color: '#ffffff',
                    borderRadius: '6px',
                    fontSize: '13px',
                    fontWeight: 600,
                    border: 'none',
                    cursor: promoteMutation.isPending ? 'not-allowed' : 'pointer',
                    opacity: promoteMutation.isPending ? 0.7 : 1,
                  }}
                >
                  {promoteMutation.isPending ? 'Promoting...' : 'Promote to Production'}
                </button>

                <button
                  type="button"
                  onClick={() => stopMutation.mutate()}
                  disabled={stopMutation.isPending}
                  style={{
                    padding: '8px 16px',
                    backgroundColor: '#ffffff',
                    color: '#dc2626',
                    borderRadius: '6px',
                    fontSize: '13px',
                    fontWeight: 600,
                    border: '1px solid #fca5a5',
                    cursor: stopMutation.isPending ? 'not-allowed' : 'pointer',
                    opacity: stopMutation.isPending ? 0.7 : 1,
                  }}
                >
                  {stopMutation.isPending ? 'Stopping...' : 'Stop Preview'}
                </button>
              </>
            )}
          </div>
        </div>

        {/* Action Banners */}
        {actionError && (
          <div
            style={{
              padding: '12px 16px',
              backgroundColor: '#fef2f2',
              color: '#991b1b',
              border: '1px solid #fecaca',
              borderRadius: '6px',
              marginBottom: '20px',
              fontSize: '13px',
            }}
          >
            {actionError}
          </div>
        )}

        {promotedDeploymentId && (
          <div
            style={{
              padding: '14px 16px',
              backgroundColor: '#ecfdf5',
              color: '#065f46',
              border: '1px solid #a7f3d0',
              borderRadius: '6px',
              marginBottom: '20px',
              fontSize: '13px',
            }}
          >
            <strong>Preview promoted to production!</strong> A new production deployment was queued from commit{' '}
            <code>{preview.head_sha.slice(0, 7)}</code>.{' '}
            <Link
              href={`/projects/${projectId}/deployments/${promotedDeploymentId}`}
              style={{ color: '#047857', fontWeight: 600, textDecoration: 'underline', marginLeft: '6px' }}
            >
              View Production Deployment &rarr;
            </Link>
          </div>
        )}

        {/* Explicit Closed / Expired State Callout */}
        {isClosed && (
          <div
            style={{
              padding: '16px 20px',
              backgroundColor: '#f9fafb',
              border: '1px solid #e5e7eb',
              borderRadius: '8px',
              marginBottom: '24px',
              display: 'flex',
              alignItems: 'center',
              gap: '14px',
            }}
          >
            <div
              style={{
                width: '36px',
                height: '36px',
                borderRadius: '50%',
                backgroundColor: '#e5e7eb',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: '#4b5563',
                fontSize: '18px',
                flexShrink: 0,
              }}
            >
              &#10005;
            </div>
            <div>
              <h3 style={{ margin: 0, fontSize: '15px', fontWeight: 600, color: '#111827' }}>
                Preview Closed and Teardown Complete
              </h3>
              <p style={{ margin: '4px 0 0 0', fontSize: '13px', color: '#6b7280' }}>
                This pull request preview has been closed and its runtime route stopped. Historical logs and build
                artifacts remain accessible below.
                {preview.closed_at && ` Closed at: ${new Date(preview.closed_at).toLocaleString()}.`}
              </p>
            </div>
          </div>
        )}

        {/* Details Grid */}
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '20px', marginBottom: '24px' }}>
          {/* PR & Git Details */}
          <div
            style={{
              backgroundColor: '#ffffff',
              border: '1px solid #e5e7eb',
              borderRadius: '8px',
              padding: '20px',
            }}
          >
            <h2 style={{ margin: '0 0 16px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
              Git & Pull Request
            </h2>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', fontSize: '13px' }}>
              <div>
                <span style={{ color: '#6b7280', display: 'block', marginBottom: '2px' }}>Pull Request</span>
                <span style={{ fontWeight: 600, color: '#111827' }}>
                  PR #{preview.pr_number} ({preview.provider})
                </span>
              </div>
              <div>
                <span style={{ color: '#6b7280', display: 'block', marginBottom: '2px' }}>Branches</span>
                <span style={{ fontFamily: 'monospace', color: '#111827' }}>
                  {preview.head_branch} &rarr; {preview.base_branch}
                </span>
              </div>
              <div>
                <span style={{ color: '#6b7280', display: 'block', marginBottom: '2px' }}>Commit SHA</span>
                <span style={{ fontFamily: 'monospace', color: '#111827' }}>{preview.head_sha}</span>
              </div>
            </div>
          </div>

          {/* Environment & Runtime Details */}
          <div
            style={{
              backgroundColor: '#ffffff',
              border: '1px solid #e5e7eb',
              borderRadius: '8px',
              padding: '20px',
            }}
          >
            <h2 style={{ margin: '0 0 16px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
              Preview Environment
            </h2>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', fontSize: '13px' }}>
              <div>
                <span style={{ color: '#6b7280', display: 'block', marginBottom: '2px' }}>Hostname</span>
                <span style={{ fontFamily: 'monospace', color: isClosed ? '#9ca3af' : '#2563eb' }}>
                  {preview.hostname}
                </span>
              </div>
              <div>
                <span style={{ color: '#6b7280', display: 'block', marginBottom: '2px' }}>Environment Scope</span>
                <span
                  style={{
                    padding: '2px 8px',
                    borderRadius: '4px',
                    backgroundColor: '#eff6ff',
                    color: '#1e40af',
                    fontWeight: 600,
                    fontSize: '12px',
                    display: 'inline-block',
                  }}
                >
                  preview (isolated from production)
                </span>
              </div>
              <div>
                <span style={{ color: '#6b7280', display: 'block', marginBottom: '2px' }}>Created / Updated</span>
                <span style={{ color: '#4b5563' }}>
                  {new Date(preview.created_at).toLocaleString()} (Updated:{' '}
                  {new Date(preview.updated_at).toLocaleString()})
                </span>
              </div>
            </div>
          </div>
        </div>

        {/* Associated Deployment & Logs */}
        <div
          style={{
            backgroundColor: '#ffffff',
            border: '1px solid #e5e7eb',
            borderRadius: '8px',
            padding: '20px',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div>
              <h2 style={{ margin: '0 0 4px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
                Build & Runtime Deployment
              </h2>
              <p style={{ margin: 0, fontSize: '13px', color: '#6b7280' }}>
                {preview.deployment_id
                  ? `Active preview deployment ID: ${preview.deployment_id}`
                  : 'No active deployment record.'}
              </p>
            </div>
            {preview.deployment_id && (
              <Link
                href={`/projects/${projectId}/deployments/${preview.deployment_id}`}
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#f3f4f6',
                  color: '#374151',
                  borderRadius: '6px',
                  fontSize: '13px',
                  fontWeight: 600,
                  textDecoration: 'none',
                  border: '1px solid #d1d5db',
                }}
              >
                Inspect Live Build & Logs &rarr;
              </Link>
            )}
          </div>
        </div>
      </div>
    </AppShell>
  );
}
