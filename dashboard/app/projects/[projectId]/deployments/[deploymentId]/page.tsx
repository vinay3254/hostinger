'use client';

import React, { useState } from 'react';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from '../../../../../lib/api';
import { AppShell } from '../../../../../components/AppShell';
import { StatusBadge } from '../../../../../components/StatusBadge';
import { LogViewer } from '../../../../../components/LogViewer';
import { EmptyState } from '../../../../../components/EmptyState';
import { ReleaseProgress } from '../../../../../components/ReleaseProgress';
import { RollbackDialog } from '../../../../../components/RollbackDialog';

export default function DeploymentDetailPage() {
  const params = useParams();
  const router = useRouter();
  const queryClient = useQueryClient();
  const projectId = params.projectId as string;
  const deploymentId = params.deploymentId as string;
  const [stopping, setStopping] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [isRollbackOpen, setIsRollbackOpen] = useState(false);

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const { data: deployment, isLoading: depLoading } = useQuery({
    queryKey: ['deployment', deploymentId],
    queryFn: () => api.getDeployment(deploymentId),
    enabled: Boolean(deploymentId),
    refetchInterval: (query) => {
      const data = query.state.data;
      if (
        data &&
        (data.status === 'building' ||
          data.status === 'pending' ||
          data.status === 'queued' ||
          data.status === 'retrying')
      ) {
        return 1000;
      }
      return false;
    },
  });

  const isBuildingOrQueued = Boolean(
    deployment &&
      ['queued', 'pending', 'building', 'retrying'].includes(deployment.status)
  );

  const {
    data: logsData,
    isError: isLogsError,
  } = useQuery({
    queryKey: ['logs', deploymentId],
    queryFn: () => api.getDeploymentLogs(deploymentId),
    enabled: Boolean(deploymentId),
    refetchInterval: isBuildingOrQueued
      ? 1000
      : deployment?.status === 'running'
      ? 3000
      : false,
  });

  const stopMutation = useMutation({
    mutationFn: () => api.stopDeployment(deploymentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['deployment', deploymentId] });
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
    },
  });

  const cancelMutation = useMutation({
    mutationFn: () => api.cancelDeployment(deploymentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['deployment', deploymentId] });
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
    },
  });

  const retryMutation = useMutation({
    mutationFn: () => api.retryDeployment(deploymentId),
    onSuccess: (newDep) => {
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
      if (newDep?.id && newDep.id !== deploymentId) {
        router.push(`/projects/${projectId}/deployments/${newDep.id}`);
      } else {
        queryClient.invalidateQueries({ queryKey: ['deployment', deploymentId] });
      }
    },
  });

  const redeployMutation = useMutation({
    mutationFn: () => api.createDeployment(projectId),
    onSuccess: (newDep) => {
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
      router.push(`/projects/${projectId}/deployments/${newDep.id}`);
    },
  });

  const handleStop = async () => {
    setStopping(true);
    try {
      await stopMutation.mutateAsync();
    } finally {
      setStopping(false);
    }
  };

  const handleCancel = async () => {
    if (
      typeof window !== 'undefined' &&
      !window.confirm('Are you sure you want to cancel this build?')
    ) {
      return;
    }
    setCancelling(true);
    try {
      await cancelMutation.mutateAsync();
    } finally {
      setCancelling(false);
    }
  };

  const handleRetry = async () => {
    setRetrying(true);
    try {
      await retryMutation.mutateAsync();
    } finally {
      setRetrying(false);
    }
  };

  const handleRedeploy = async () => {
    await redeployMutation.mutateAsync();
  };

  if (depLoading) {
    return (
      <AppShell currentProjectId={projectId}>
        <div style={{ padding: '40px', textAlign: 'center', color: '#6b7280' }}>Loading deployment...</div>
      </AppShell>
    );
  }

  if (!deployment) {
    return (
      <AppShell currentProjectId={projectId}>
        <EmptyState
          title="Deployment not found"
          description="The requested deployment could not be loaded."
          actionLabel="Back to Deployments"
          actionHref={`/projects/${projectId}/deployments`}
        />
      </AppShell>
    );
  }

  return (
    <AppShell currentProjectId={projectId}>
      {/* Breadcrumb */}
      <div style={{ fontSize: '13px', color: '#6b7280', marginBottom: '16px' }}>
        <Link href={`/projects/${projectId}/deployments`} style={{ color: '#2563eb', textDecoration: 'none' }}>
          ← Deployments
        </Link>
        <span style={{ margin: '0 8px' }}>/</span>
        <span style={{ fontFamily: 'monospace' }}>{deployment.id}</span>
      </div>

      {/* Header */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '24px' }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
            <h1 style={{ margin: 0, fontSize: '22px', fontWeight: 700, fontFamily: 'monospace', color: '#111827' }}>
              {deployment.id}
            </h1>
            <StatusBadge status={deployment.status} />
          </div>
          {deployment.url && (
            <div style={{ marginTop: '6px' }}>
              <a
                href={deployment.url}
                target="_blank"
                rel="noopener noreferrer"
                style={{ fontSize: '14px', color: '#2563eb', textDecoration: 'none', fontWeight: 500 }}
              >
                {deployment.url} ↗
              </a>
            </div>
          )}
        </div>

        <div style={{ display: 'flex', gap: '10px' }}>
          {(deployment.status === 'queued' ||
            deployment.status === 'building' ||
            deployment.status === 'retrying') && (
            <button
              onClick={handleCancel}
              disabled={cancelling}
              style={{
                padding: '8px 16px',
                borderRadius: '6px',
                border: '1px solid #dc2626',
                backgroundColor: '#ffffff',
                color: '#dc2626',
                fontSize: '13px',
                fontWeight: 500,
                cursor: cancelling ? 'not-allowed' : 'pointer',
              }}
            >
              {cancelling ? 'Cancelling...' : 'Cancel Build'}
            </button>
          )}

          {(deployment.status === 'failed' || deployment.status === 'cancelled') && (
            <button
              onClick={handleRetry}
              disabled={retrying}
              style={{
                padding: '8px 16px',
                borderRadius: '6px',
                border: '1px solid #2563eb',
                backgroundColor: '#ffffff',
                color: '#2563eb',
                fontSize: '13px',
                fontWeight: 500,
                cursor: retrying ? 'not-allowed' : 'pointer',
              }}
            >
              {retrying ? 'Retrying...' : 'Retry Build'}
            </button>
          )}

          {deployment.status === 'running' && (
            <button
              onClick={handleStop}
              disabled={stopping}
              style={{
                padding: '8px 16px',
                borderRadius: '6px',
                border: '1px solid #dc2626',
                backgroundColor: '#ffffff',
                color: '#dc2626',
                fontSize: '13px',
                fontWeight: 500,
                cursor: stopping ? 'not-allowed' : 'pointer',
              }}
            >
              {stopping ? 'Stopping...' : 'Stop Deployment'}
            </button>
          )}

          <button
            onClick={() => setIsRollbackOpen(true)}
            data-testid="rollback-button"
            style={{
              padding: '8px 16px',
              borderRadius: '6px',
              border: '1px solid #e11d48',
              backgroundColor: '#fff1f2',
              color: '#e11d48',
              fontSize: '13px',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            Rollback
          </button>

          <button
            onClick={handleRedeploy}
            style={{
              padding: '8px 16px',
              borderRadius: '6px',
              border: 'none',
              backgroundColor: '#111827',
              color: '#ffffff',
              fontSize: '13px',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            Redeploy
          </button>
        </div>
      </div>

      {/* Metadata Grid */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
          gap: '16px',
          backgroundColor: '#ffffff',
          border: '1px solid #e5e7eb',
          borderRadius: '8px',
          padding: '20px',
          marginBottom: '24px',
          boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
        }}
      >
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Framework</span>
          <span style={{ fontSize: '14px', fontWeight: 500, textTransform: 'capitalize' }}>{deployment.framework}</span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Commit SHA</span>
          <span style={{ fontSize: '14px', fontWeight: 500, fontFamily: 'monospace' }}>
            {deployment.commit_sha ? deployment.commit_sha.substring(0, 7) : 'None'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Attempt</span>
          <span style={{ fontSize: '14px', fontWeight: 500 }}>
            {deployment.attempt ? `Attempt ${deployment.attempt}` : 'Attempt 1'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Worker Node</span>
          <span style={{ fontSize: '14px', fontWeight: 500, fontFamily: 'monospace' }}>
            {deployment.worker_id || 'None'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Queue Wait</span>
          <span style={{ fontSize: '14px', fontWeight: 500 }}>
            {deployment.queue_wait_ms != null ? `${deployment.queue_wait_ms}ms` : '0ms'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Cache Status</span>
          <span
            style={{
              display: 'inline-block',
              padding: '2px 8px',
              borderRadius: '4px',
              fontSize: '12px',
              fontWeight: 600,
              fontFamily: 'monospace',
              backgroundColor: deployment.cache_status === 'HIT' ? '#dcfce7' : '#f3f4f6',
              color: deployment.cache_status === 'HIT' ? '#15803d' : '#4b5563',
            }}
          >
            {deployment.cache_status || 'MISS'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Cache Key</span>
          <span
            title={deployment.cache_key || 'No cache key'}
            style={{ fontSize: '14px', fontWeight: 500, fontFamily: 'monospace' }}
          >
            {deployment.cache_key
              ? `${deployment.cache_key.substring(0, 16)}...`
              : 'None'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Build Duration</span>
          <span style={{ fontSize: '14px', fontWeight: 500 }}>
            {deployment.build_duration_ms != null
              ? `${(deployment.build_duration_ms / 1000).toFixed(1)}s`
              : 'N/A'}
            {deployment.cached_duration_ms != null && (
              <span style={{ fontSize: '12px', color: '#10b981', marginLeft: '6px' }}>
                (cached ~{(deployment.cached_duration_ms / 1000).toFixed(1)}s)
              </span>
            )}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Artifact Metadata</span>
          <span style={{ fontSize: '14px', fontWeight: 500 }}>
            {deployment.artifact_size_bytes != null
              ? `${(deployment.artifact_size_bytes / 1024 / 1024).toFixed(2)} MB`
              : deployment.image_path ? 'Packaged' : 'None'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Container Port</span>
          <span style={{ fontSize: '14px', fontWeight: 500, fontFamily: 'monospace' }}>
            {deployment.port ? `:${deployment.port}` : 'None'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Created</span>
          <span style={{ fontSize: '14px', fontWeight: 500 }}>
            {new Date(deployment.created_at).toLocaleString()}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Container ID</span>
          <span style={{ fontSize: '14px', fontWeight: 500, fontFamily: 'monospace' }}>
            {deployment.container_id ? deployment.container_id.substring(0, 12) : 'None'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '12px', color: '#6b7280', display: 'block', marginBottom: '4px' }}>Cause</span>
          <span style={{ fontSize: '14px', fontWeight: 500, textTransform: 'capitalize' }}>
            {deployment.cause === 'rollback' ? (
              <span style={{ color: '#e11d48', fontWeight: 600 }}>Rollback</span>
            ) : (
              deployment.cause || 'Manual'
            )}
          </span>
        </div>
      </div>

      {/* Release Pipeline Progress */}
      <div style={{ marginBottom: '24px' }}>
        <ReleaseProgress deploymentId={deploymentId} />
      </div>

      {deployment.error && (
        <div
          role="alert"
          style={{
            marginBottom: '24px',
            padding: '16px',
            borderRadius: '8px',
            backgroundColor: '#fef2f2',
            border: '1px solid #fecaca',
            color: '#991b1b',
            fontSize: '14px',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            gap: '16px',
          }}
        >
          <div>
            <strong>Deployment Error:</strong> {deployment.error}
            <div style={{ marginTop: '4px', fontSize: '12px', color: '#7f1d1d' }}>
              Inspect build logs below for detailed diagnostic messages or retry the build.
            </div>
          </div>
          <button
            onClick={handleRetry}
            disabled={retrying}
            style={{
              padding: '6px 12px',
              borderRadius: '6px',
              border: '1px solid #b91c1c',
              backgroundColor: '#ffffff',
              color: '#b91c1c',
              fontSize: '12px',
              fontWeight: 500,
              cursor: retrying ? 'not-allowed' : 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {retrying ? 'Retrying...' : 'Retry Build'}
          </button>
        </div>
      )}

      {/* Logs Section */}
      <section>
        <h2 style={{ fontSize: '16px', fontWeight: 600, color: '#111827', marginBottom: '12px' }}>
          Deployment Logs
        </h2>
        <LogViewer
          logs={logsData?.logs || ''}
          deploymentId={deploymentId}
          isStreaming={isBuildingOrQueued && !isLogsError}
          isReconnecting={isBuildingOrQueued && isLogsError}
        />
      </section>

      {/* Rollback Dialog */}
      <RollbackDialog
        isOpen={isRollbackOpen}
        onClose={() => setIsRollbackOpen(false)}
        projectId={projectId}
        deploymentId={deploymentId}
        currentCommitSha={deployment.commit_sha}
      />
    </AppShell>
  );
}

