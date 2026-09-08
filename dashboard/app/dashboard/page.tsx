'use client';

import React, { useEffect } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../lib/api';
import { AppShell } from '../../components/AppShell';
import { EmptyState } from '../../components/EmptyState';
import { StatusBadge } from '../../components/StatusBadge';

export default function DashboardPage() {
  const router = useRouter();

  const { data: user, isLoading: userLoading, isError: userError } = useQuery({
    queryKey: ['me'],
    queryFn: () => api.me(),
    retry: false,
  });

  const { data: projects = [], isLoading: projectsLoading } = useQuery({
    queryKey: ['projects'],
    queryFn: () => api.listProjects(),
    enabled: Boolean(user),
  });

  useEffect(() => {
    if (!userLoading && (userError || !user)) {
      router.push('/login');
    }
  }, [user, userLoading, userError, router]);

  if (userLoading || projectsLoading) {
    return (
      <AppShell>
        <div style={{ textAlign: 'center', padding: '60px', color: '#6b7280' }}>Loading dashboard...</div>
      </AppShell>
    );
  }

  return (
    <AppShell>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
        <div>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>Dashboard</h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Account-wide project and deployment overview
          </p>
        </div>
        <Link
          href="/projects/new"
          style={{
            padding: '8px 16px',
            backgroundColor: '#111827',
            color: '#ffffff',
            borderRadius: '6px',
            fontSize: '14px',
            fontWeight: 500,
            textDecoration: 'none',
          }}
        >
          + New Project
        </Link>
      </div>

      {projects.length === 0 ? (
        <EmptyState
          title="Create your first project"
          description="Deploy static web applications with container isolation in seconds."
          actionLabel="Create Project"
          actionHref="/projects/new"
        />
      ) : (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: '20px' }}>
          {projects.map((project) => (
            <Link
              key={project.id}
              href={`/projects/${project.id}/overview`}
              style={{
                display: 'block',
                padding: '20px',
                backgroundColor: '#ffffff',
                border: '1px solid #e5e7eb',
                borderRadius: '8px',
                textDecoration: 'none',
                boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
                transition: 'border-color 0.15s ease',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
                <h3 style={{ margin: 0, fontSize: '16px', fontWeight: 600, color: '#111827' }}>
                  {project.name}
                </h3>
                {project.active_deployment ? (
                  <StatusBadge status="running" />
                ) : (
                  <StatusBadge status="stopped" />
                )}
              </div>
              <div style={{ fontSize: '13px', color: '#6b7280', display: 'flex', flexDirection: 'column', gap: '6px' }}>
                <div>
                  <span style={{ color: '#9ca3af' }}>Source: </span>
                  <span style={{ fontFamily: 'monospace' }}>{project.source_dir}</span>
                </div>
                <div>
                  <span style={{ color: '#9ca3af' }}>Created: </span>
                  <time dateTime={project.created_at}>
                    {new Date(project.created_at).toLocaleDateString()}
                  </time>
                </div>
              </div>
            </Link>
          ))}
        </div>
      )}
    </AppShell>
  );
}
