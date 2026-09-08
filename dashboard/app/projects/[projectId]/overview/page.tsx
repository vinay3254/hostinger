'use client';

import React, { useState } from 'react';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from '../../../../lib/api';
import { AppShell } from '../../../../components/AppShell';
import { DeploymentCard } from '../../../../components/DeploymentCard';
import { EmptyState } from '../../../../components/EmptyState';
import { StatusBadge } from '../../../../components/StatusBadge';

export default function ProjectOverviewPage() {
  const params = useParams();
  const router = useRouter();
  const queryClient = useQueryClient();
  const projectId = params.projectId as string;
  const [deploying, setDeploying] = useState(false);

  const { data: project, isLoading: projectLoading } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const { data: deployments = [], isLoading: deploymentsLoading } = useQuery({
    queryKey: ['deployments', projectId],
    queryFn: () => api.listDeployments(projectId),
    enabled: Boolean(projectId),
  });

  const deployMutation = useMutation({
    mutationFn: () => api.createDeployment(projectId),
    onSuccess: (newDep) => {
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      router.push(`/projects/${projectId}/deployments/${newDep.id}`);
    },
  });

  const handleDeploy = async () => {
    setDeploying(true);
    try {
      await deployMutation.mutateAsync();
    } finally {
      setDeploying(false);
    }
  };

  if (projectLoading || deploymentsLoading) {
    return (
      <AppShell currentProjectId={projectId}>
        <div style={{ padding: '40px', textAlign: 'center', color: '#6b7280' }}>Loading project overview...</div>
      </AppShell>
    );
  }

  if (!project) {
    return (
      <AppShell currentProjectId={projectId}>
        <EmptyState
          title="Project not found"
          description="The requested project does not exist or you do not have permission to view it."
          actionLabel="Back to Projects"
          actionHref="/projects"
        />
      </AppShell>
    );
  }

  const latestDeployment = deployments.length > 0 ? deployments[0] : null;

  return (
    <AppShell currentProjectId={projectId}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: '28px' }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
            <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
              {project.name}
            </h1>
            {project.active_deployment ? (
              <StatusBadge status="running" />
            ) : (
              <StatusBadge status="stopped" />
            )}
          </div>
          <p style={{ margin: '6px 0 0 0', fontSize: '13px', color: '#6b7280', fontFamily: 'monospace' }}>
            {project.source_dir}
          </p>
        </div>

        <div style={{ display: 'flex', gap: '12px' }}>
          <button
            onClick={handleDeploy}
            disabled={deploying}
            style={{
              padding: '8px 16px',
              backgroundColor: '#111827',
              color: '#ffffff',
              borderRadius: '6px',
              fontSize: '14px',
              fontWeight: 500,
              border: 'none',
              cursor: deploying ? 'not-allowed' : 'pointer',
              opacity: deploying ? 0.7 : 1,
            }}
          >
            {deploying ? 'Deploying...' : 'Deploy Now'}
          </button>
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: '24px' }}>
        {/* Latest Deployment Section */}
        <section
          style={{
            backgroundColor: '#ffffff',
            border: '1px solid #e5e7eb',
            borderRadius: '8px',
            padding: '24px',
            boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
            <h2 style={{ margin: 0, fontSize: '16px', fontWeight: 600, color: '#111827' }}>
              Latest Deployment
            </h2>
            {deployments.length > 0 && (
              <Link
                href={`/projects/${projectId}/deployments`}
                style={{ fontSize: '13px', color: '#2563eb', textDecoration: 'none', fontWeight: 500 }}
              >
                View all ({deployments.length}) →
              </Link>
            )}
          </div>

          {latestDeployment ? (
            <DeploymentCard deployment={latestDeployment} projectId={projectId} />
          ) : (
            <EmptyState
              title="No deployments yet"
              description="This project has not been deployed yet. Trigger a deployment to launch your application."
              actionLabel="Deploy Project"
              onAction={handleDeploy}
            />
          )}
        </section>

        {/* Project Configuration Details */}
        <section
          style={{
            backgroundColor: '#ffffff',
            border: '1px solid #e5e7eb',
            borderRadius: '8px',
            padding: '24px',
            boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
          }}
        >
          <h2 style={{ margin: '0 0 16px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
            Configuration & Environment
          </h2>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', fontSize: '13px' }}>
            <div>
              <span style={{ color: '#6b7280', display: 'block', marginBottom: '4px' }}>Base Image:</span>
              <span style={{ fontFamily: 'monospace', color: '#111827' }}>{project.base_image}</span>
            </div>
            <div>
              <span style={{ color: '#6b7280', display: 'block', marginBottom: '4px' }}>Server Command:</span>
              <span style={{ fontFamily: 'monospace', color: '#111827' }}>
                {project.server_command.join(' ')}
              </span>
            </div>
          </div>
        </section>
      </div>
    </AppShell>
  );
}
