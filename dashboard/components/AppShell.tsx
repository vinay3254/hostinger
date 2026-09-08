'use client';

import React from 'react';
import Link from 'next/link';
import { usePathname, useRouter } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api } from '../lib/api';
import { ProjectSwitcher } from './ProjectSwitcher';

interface AppShellProps {
  children: React.ReactNode;
  currentProjectId?: string;
}

export const AppShell: React.FC<AppShellProps> = ({ children, currentProjectId }) => {
  const router = useRouter();
  const pathname = usePathname();

  const { data: user } = useQuery({
    queryKey: ['me'],
    queryFn: () => api.me(),
    retry: false,
  });

  const { data: projects = [] } = useQuery({
    queryKey: ['projects'],
    queryFn: () => api.listProjects(),
    retry: false,
  });

  const handleLogout = async () => {
    try {
      await api.logout();
    } catch {
      // ignore
    }
    router.push('/login');
  };

  const isProjectRoute = Boolean(currentProjectId);

  return (
    <div style={{ minHeight: '100vh', display: 'flex', flexDirection: 'column', backgroundColor: '#f9fafb', color: '#111827', fontFamily: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif' }}>
      {/* Top Header */}
      <header
        style={{
          height: '60px',
          borderBottom: '1px solid #e5e7eb',
          backgroundColor: '#ffffff',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '0 24px',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '20px' }}>
          <Link
            href="/dashboard"
            style={{
              fontWeight: 700,
              fontSize: '16px',
              color: '#111827',
              textDecoration: 'none',
              display: 'flex',
              alignItems: 'center',
              gap: '8px',
            }}
          >
            <span
              style={{
                width: '18px',
                height: '18px',
                backgroundColor: '#111827',
                borderRadius: '4px',
                display: 'inline-block',
              }}
            />
            DeployPlatform
          </Link>

          {projects.length > 0 && (
            <ProjectSwitcher projects={projects} currentProjectId={currentProjectId} />
          )}
        </div>

        <nav style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
          <Link
            href="/dashboard"
            style={{
              fontSize: '14px',
              color: pathname === '/dashboard' ? '#111827' : '#4b5563',
              fontWeight: pathname === '/dashboard' ? 600 : 400,
              textDecoration: 'none',
            }}
          >
            Dashboard
          </Link>
          <Link
            href="/projects"
            style={{
              fontSize: '14px',
              color: pathname === '/projects' ? '#111827' : '#4b5563',
              fontWeight: pathname === '/projects' ? 600 : 400,
              textDecoration: 'none',
            }}
          >
            Projects
          </Link>
          <Link
            href="/account/api-tokens"
            style={{
              fontSize: '14px',
              color: pathname === '/account/api-tokens' ? '#111827' : '#4b5563',
              fontWeight: pathname === '/account/api-tokens' ? 600 : 400,
              textDecoration: 'none',
            }}
          >
            API Tokens
          </Link>

          {user && (
            <div style={{ display: 'flex', alignItems: 'center', gap: '12px', borderLeft: '1px solid #e5e7eb', paddingLeft: '16px' }}>
              <span style={{ fontSize: '13px', color: '#6b7280' }}>{user.email}</span>
              <button
                onClick={handleLogout}
                style={{
                  padding: '4px 10px',
                  borderRadius: '6px',
                  border: '1px solid #d1d5db',
                  backgroundColor: '#ffffff',
                  fontSize: '13px',
                  cursor: 'pointer',
                  color: '#374151',
                }}
              >
                Logout
              </button>
            </div>
          )}
        </nav>
      </header>

      {/* Project Subnav (when inside a project) */}
      {isProjectRoute && (
        <div
          style={{
            height: '44px',
            borderBottom: '1px solid #e5e7eb',
            backgroundColor: '#ffffff',
            display: 'flex',
            alignItems: 'center',
            padding: '0 24px',
            gap: '24px',
          }}
        >
          <Link
            href={`/projects/${currentProjectId}/overview`}
            style={{
              fontSize: '13px',
              fontWeight: pathname.includes('/overview') ? 600 : 400,
              color: pathname.includes('/overview') ? '#111827' : '#6b7280',
              borderBottom: pathname.includes('/overview') ? '2px solid #111827' : 'none',
              padding: '12px 0',
              textDecoration: 'none',
            }}
          >
            Overview
          </Link>
          <Link
            href={`/projects/${currentProjectId}/deployments`}
            style={{
              fontSize: '13px',
              fontWeight: pathname.includes('/deployments') ? 600 : 400,
              color: pathname.includes('/deployments') ? '#111827' : '#6b7280',
              borderBottom: pathname.includes('/deployments') ? '2px solid #111827' : 'none',
              padding: '12px 0',
              textDecoration: 'none',
            }}
          >
            Deployments
          </Link>
          <Link
            href={`/projects/${currentProjectId}/environment`}
            style={{
              fontSize: '13px',
              fontWeight: pathname.includes('/environment') ? 600 : 400,
              color: pathname.includes('/environment') ? '#111827' : '#6b7280',
              borderBottom: pathname.includes('/environment') ? '2px solid #111827' : 'none',
              padding: '12px 0',
              textDecoration: 'none',
            }}
          >
            Environment
          </Link>
        </div>
      )}

      {/* Main Content Area */}
      <main style={{ flex: 1, padding: '32px 24px', maxWidth: '1200px', width: '100%', margin: '0 auto', boxSizing: 'border-box' }}>
        {children}
      </main>
    </div>
  );
};
