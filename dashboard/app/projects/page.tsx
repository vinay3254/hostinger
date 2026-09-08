'use client';

import React, { useState } from 'react';
import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../lib/api';
import { AppShell } from '../../components/AppShell';
import { EmptyState } from '../../components/EmptyState';
import { StatusBadge } from '../../components/StatusBadge';

export default function ProjectsPage() {
  const [search, setSearch] = useState('');

  const { data: projects = [], isLoading } = useQuery({
    queryKey: ['projects'],
    queryFn: () => api.listProjects(),
  });

  const filtered = projects.filter((p) =>
    p.name.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <AppShell>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
        <div>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>Projects</h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Manage and observe your deployed static applications
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

      <div style={{ marginBottom: '20px' }}>
        <input
          type="text"
          placeholder="Search projects..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{
            padding: '8px 12px',
            borderRadius: '6px',
            border: '1px solid #d1d5db',
            fontSize: '14px',
            width: '100%',
            maxWidth: '320px',
          }}
        />
      </div>

      {isLoading ? (
        <div style={{ padding: '40px 0', textAlign: 'center', color: '#6b7280' }}>Loading projects...</div>
      ) : filtered.length === 0 ? (
        <EmptyState
          title={search ? 'No matching projects' : 'No projects yet'}
          description={search ? 'Try adjusting your search criteria.' : 'Create your first project to begin deploying.'}
          actionLabel={search ? undefined : 'Create Project'}
          actionHref={search ? undefined : '/projects/new'}
        />
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
          {filtered.map((p) => (
            <div
              key={p.id}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                padding: '16px 20px',
                backgroundColor: '#ffffff',
                border: '1px solid #e5e7eb',
                borderRadius: '8px',
                boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
              }}
            >
              <div>
                <Link
                  href={`/projects/${p.id}/overview`}
                  style={{ fontSize: '16px', fontWeight: 600, color: '#111827', textDecoration: 'none' }}
                >
                  {p.name}
                </Link>
                <div style={{ fontSize: '13px', color: '#6b7280', marginTop: '4px' }}>
                  Source: <span style={{ fontFamily: 'monospace' }}>{p.source_dir}</span>
                </div>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
                {p.active_deployment ? (
                  <StatusBadge status="running" />
                ) : (
                  <StatusBadge status="stopped" />
                )}
                <Link
                  href={`/projects/${p.id}/overview`}
                  style={{
                    fontSize: '13px',
                    color: '#2563eb',
                    textDecoration: 'none',
                    fontWeight: 500,
                  }}
                >
                  View →
                </Link>
              </div>
            </div>
          ))}
        </div>
      )}
    </AppShell>
  );
}
