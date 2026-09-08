'use client';

import React, { useState } from 'react';
import { useRouter } from 'next/navigation';
import { api, ApiError } from '../../../lib/api';
import { AppShell } from '../../../components/AppShell';

export default function NewProjectPage() {
  const router = useRouter();
  const [name, setName] = useState('');
  const [sourceDir, setSourceDir] = useState('');
  const [baseImage, setBaseImage] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setLoading(true);

    try {
      const project = await api.createProject({
        name: name.trim(),
        source_dir: sourceDir.trim(),
        base_image: baseImage.trim(),
      });
      router.push(`/projects/${project.id}/overview`);
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message);
      } else {
        setError('Failed to create project.');
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <AppShell>
      <div style={{ maxWidth: '600px', margin: '0 auto' }}>
        <div style={{ marginBottom: '24px' }}>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>Create New Project</h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Configure source code and rootfs base image for deployment
          </p>
        </div>

        {error && (
          <div
            role="alert"
            style={{
              marginBottom: '20px',
              padding: '12px 16px',
              borderRadius: '6px',
              backgroundColor: '#fef2f2',
              border: '1px solid #fecaca',
              color: '#991b1b',
              fontSize: '13px',
            }}
          >
            {error}
          </div>
        )}

        <form
          onSubmit={handleSubmit}
          style={{
            backgroundColor: '#ffffff',
            border: '1px solid #e5e7eb',
            borderRadius: '8px',
            padding: '24px',
            boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
            display: 'flex',
            flexDirection: 'column',
            gap: '20px',
          }}
        >
          <div>
            <label
              htmlFor="project-name"
              style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
            >
              Project Name
            </label>
            <input
              id="project-name"
              type="text"
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. my-landing-page"
              style={{
                width: '100%',
                padding: '8px 12px',
                borderRadius: '6px',
                border: '1px solid #d1d5db',
                fontSize: '14px',
                boxSizing: 'border-box',
              }}
            />
            <span style={{ fontSize: '12px', color: '#6b7280', marginTop: '4px', display: 'block' }}>
              Unique alphanumeric name for your project
            </span>
          </div>

          <div>
            <label
              htmlFor="source-dir"
              style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
            >
              Source Directory
            </label>
            <input
              id="source-dir"
              type="text"
              required
              value={sourceDir}
              onChange={(e) => setSourceDir(e.target.value)}
              placeholder="/absolute/path/to/dist or html files"
              style={{
                width: '100%',
                padding: '8px 12px',
                borderRadius: '6px',
                border: '1px solid #d1d5db',
                fontSize: '14px',
                boxSizing: 'border-box',
                fontFamily: 'monospace',
              }}
            />
            <span style={{ fontSize: '12px', color: '#6b7280', marginTop: '4px', display: 'block' }}>
              Absolute directory path containing index.html
            </span>
          </div>

          <div>
            <label
              htmlFor="base-image"
              style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
            >
              Base Image (.tar.gz)
            </label>
            <input
              id="base-image"
              type="text"
              required
              value={baseImage}
              onChange={(e) => setBaseImage(e.target.value)}
              placeholder="/path/to/minidock-base.tar.gz"
              style={{
                width: '100%',
                padding: '8px 12px',
                borderRadius: '6px',
                border: '1px solid #d1d5db',
                fontSize: '14px',
                boxSizing: 'border-box',
                fontFamily: 'monospace',
              }}
            />
            <span style={{ fontSize: '12px', color: '#6b7280', marginTop: '4px', display: 'block' }}>
              Base rootfs image containing http server binary
            </span>
          </div>

          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '12px', marginTop: '8px' }}>
            <button
              type="button"
              onClick={() => router.back()}
              style={{
                padding: '8px 16px',
                borderRadius: '6px',
                border: '1px solid #d1d5db',
                backgroundColor: '#ffffff',
                fontSize: '14px',
                cursor: 'pointer',
                color: '#374151',
              }}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={loading}
              style={{
                padding: '8px 16px',
                borderRadius: '6px',
                border: 'none',
                backgroundColor: '#111827',
                color: '#ffffff',
                fontSize: '14px',
                fontWeight: 500,
                cursor: loading ? 'not-allowed' : 'pointer',
                opacity: loading ? 0.7 : 1,
              }}
            >
              {loading ? 'Creating...' : 'Create Project'}
            </button>
          </div>
        </form>
      </div>
    </AppShell>
  );
}
