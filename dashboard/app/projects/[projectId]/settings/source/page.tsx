'use client';

import React from 'react';
import { useParams } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../../../../lib/api';
import { AppShell } from '../../../../../components/AppShell';
import { SourceConnectionCard } from '../../../../../components/SourceConnectionCard';

export default function ProjectSourceSettingsPage() {
  const params = useParams();
  const projectId = params.projectId as string;

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  return (
    <AppShell currentProjectId={projectId}>
      <div style={{ maxWidth: '800px', margin: '0 auto' }}>
        <div style={{ marginBottom: '28px' }}>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Source Settings
          </h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Connect a git repository to configure automated deployments for {project?.name || 'this project'}.
          </p>
        </div>

        <SourceConnectionCard projectId={projectId} />
      </div>
    </AppShell>
  );
}
