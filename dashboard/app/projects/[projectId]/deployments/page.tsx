'use client';

import React, { useState } from 'react';
import { useParams, useRouter } from 'next/navigation';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api, DeploymentStatus } from '../../../../lib/api';
import { AppShell } from '../../../../components/AppShell';
import { DeploymentCard } from '../../../../components/DeploymentCard';
import { EmptyState } from '../../../../components/EmptyState';

export default function ProjectDeploymentsPage() {
  const params = useParams();
  const router = useRouter();
  const queryClient = useQueryClient();
  const projectId = params.projectId as string;
  const [filter, setFilter] = useState<string>('all');
  const [deploying, setDeploying] = useState(false);

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const { data: deployments = [], isLoading } = useQuery({
    queryKey: ['deployments', projectId],
    queryFn: () => api.listDeployments(projectId),
    enabled: Boolean(projectId),
  });

  const deployMutation = useMutation({
    mutationFn: () => api.createDeployment(projectId),
    onSuccess: (newDep) => {
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
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

  const filtered = deployments.filter((d) =>
    filter === 'all' ? true : d.status === (filter as DeploymentStatus)
  );

  return (
    <AppShell currentProjectId={projectId}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
        <div>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Deployments
          </h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            {project ? `${project.name} deployment history` : 'Deployment history'}
          </p>
        </div>

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
          {deploying ? 'Deploying...' : '+ New Deployment'}
        </button>
      </div>

      <div style={{ marginBottom: '20px', display: 'flex', gap: '8px' }}>
        {['all', 'running', 'failed', 'stopped'].map((status) => (
          <button
            key={status}
            onClick={() => setFilter(status)}
            style={{
              padding: '6px 12px',
              borderRadius: '6px',
              border: filter === status ? '1px solid #111827' : '1px solid #d1d5db',
              backgroundColor: filter === status ? '#111827' : '#ffffff',
              color: filter === status ? '#ffffff' : '#374151',
              fontSize: '13px',
              cursor: 'pointer',
              textTransform: 'capitalize',
            }}
          >
            {status}
          </button>
        ))}
      </div>

      {isLoading ? (
        <div style={{ padding: '40px', textAlign: 'center', color: '#6b7280' }}>Loading deployments...</div>
      ) : filtered.length === 0 ? (
        <EmptyState
          title="No deployments found"
          description={filter === 'all' ? 'No deployments have been made for this project yet.' : `No deployments with status '${filter}'.`}
          actionLabel={filter === 'all' ? 'Trigger First Deployment' : undefined}
          onAction={filter === 'all' ? handleDeploy : undefined}
        />
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
          {filtered.map((d) => (
            <DeploymentCard key={d.id} deployment={d} projectId={projectId} />
          ))}
        </div>
      )}
    </AppShell>
  );
}
