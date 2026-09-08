'use client';

import React from 'react';
import { useRouter } from 'next/navigation';
import { Project } from '../lib/api';

interface ProjectSwitcherProps {
  projects: Project[];
  currentProjectId?: string;
}

export const ProjectSwitcher: React.FC<ProjectSwitcherProps> = ({
  projects,
  currentProjectId,
}) => {
  const router = useRouter();

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
      <label htmlFor="project-switcher" style={{ fontSize: '13px', color: '#6b7280' }}>
        Project:
      </label>
      <select
        id="project-switcher"
        value={currentProjectId || ''}
        onChange={(e) => {
          const val = e.target.value;
          if (val === 'new') {
            router.push('/projects/new');
          } else if (val) {
            router.push(`/projects/${val}/overview`);
          } else {
            router.push('/projects');
          }
        }}
        style={{
          padding: '6px 12px',
          borderRadius: '6px',
          border: '1px solid #d1d5db',
          backgroundColor: '#ffffff',
          fontSize: '13px',
          fontWeight: 500,
          color: '#111827',
          cursor: 'pointer',
        }}
      >
        <option value="">All Projects</option>
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
        <option value="new">+ Create New Project</option>
      </select>
    </div>
  );
};
