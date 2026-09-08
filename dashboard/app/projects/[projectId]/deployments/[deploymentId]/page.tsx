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

export default function DeploymentDetailPage() {
  const params = useParams();
  const router = useRouter();
  const queryClient = useQueryClient();
  const projectId = params.projectId as string;
  const deploymentId = params.deploymentId as string;
  const [stopping, setStopping] = useState(false);

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
      if (data && (data.status === 'building' || data.status === 'pending')) {
        return 1000;
      }
      return false;
    },
  });

  const { data: logsData } = useQuery({
    queryKey: ['logs', deploymentId],
    queryFn: () => api.getDeploymentLogs(deploymentId),
    enabled: Boolean(deploymentId),
    refetchInterval: deployment?.status === 'running' ? 3000 : false,
  });

  const stopMutation = useMutation({
    mutationFn: () => api.stopDeployment(deploymentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['deployment', deploymentId] });
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
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
          }}
        >
          <strong>Deployment Error:</strong> {deployment.error}
        </div>
      )}

      {/* Logs Section */}
      <section>
        <h2 style={{ fontSize: '16px', fontWeight: 600, color: '#111827', marginBottom: '12px' }}>
          Deployment Logs
        </h2>
        <LogViewer logs={logsData?.logs || ''} />
      </section>
    </AppShell>
  );
}
