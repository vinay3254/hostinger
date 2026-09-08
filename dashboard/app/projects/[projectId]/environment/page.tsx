'use client';

import React, { useState } from 'react';
import { useParams } from 'next/navigation';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../../../lib/api';
import { AppShell } from '../../../../components/AppShell';

interface EnvVar {
  key: string;
  value: string;
  isSecret: boolean;
}

export default function ProjectEnvironmentPage() {
  const params = useParams();
  const projectId = params.projectId as string;

  const [key, setKey] = useState('');
  const [value, setValue] = useState('');
  const [isSecret, setIsSecret] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Local state for environment variables in M2
  const [variables, setVariables] = useState<EnvVar[]>([
    { key: 'PORT', value: '8080', isSecret: false },
    { key: 'DATABASE_URL', value: 'postgres://...', isSecret: true },
  ]);

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const handleAdd = (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSuccess(null);

    const trimmedKey = key.trim().toUpperCase();
    if (!/^[A-Z_][A-Z0-9_]*$/.test(trimmedKey)) {
      setError('Key must contain only uppercase letters, numbers, and underscores (e.g. API_KEY).');
      return;
    }

    if (variables.some((v) => v.key === trimmedKey)) {
      setError(`Environment variable '${trimmedKey}' already exists.`);
      return;
    }

    setVariables([...variables, { key: trimmedKey, value: value.trim(), isSecret }]);
    setKey('');
    setValue('');
    setSuccess(`Environment variable '${trimmedKey}' configured successfully.`);
  };

  const handleRemove = (keyToRemove: string) => {
    setVariables(variables.filter((v) => v.key !== keyToRemove));
  };

  return (
    <AppShell currentProjectId={projectId}>
      <div style={{ maxWidth: '800px', margin: '0 auto' }}>
        <div style={{ marginBottom: '28px' }}>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Environment Variables
          </h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Configure build and runtime variables for {project?.name || 'this project'}
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

        {success && (
          <div
            role="status"
            style={{
              marginBottom: '20px',
              padding: '12px 16px',
              borderRadius: '6px',
              backgroundColor: '#ecfdf5',
              border: '1px solid #a7f3d0',
              color: '#065f46',
              fontSize: '13px',
            }}
          >
            {success}
          </div>
        )}

        {/* Add Variable Form */}
        <div
          style={{
            backgroundColor: '#ffffff',
            border: '1px solid #e5e7eb',
            borderRadius: '8px',
            padding: '20px',
            marginBottom: '32px',
            boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
          }}
        >
          <h2 style={{ margin: '0 0 16px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
            Add Environment Variable
          </h2>

          <form onSubmit={handleAdd} style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
              <div>
                <label
                  htmlFor="env-key"
                  style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
                >
                  Key
                </label>
                <input
                  id="env-key"
                  type="text"
                  required
                  placeholder="e.g. STRIPE_API_KEY"
                  value={key}
                  onChange={(e) => setKey(e.target.value.toUpperCase())}
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
              </div>

              <div>
                <label
                  htmlFor="env-val"
                  style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
                >
                  Value
                </label>
                <input
                  id="env-val"
                  type={isSecret ? 'password' : 'text'}
                  required
                  placeholder="Value"
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
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
              </div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <label style={{ display: 'flex', alignItems: 'center', gap: '8px', fontSize: '13px', color: '#374151', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={isSecret}
                  onChange={(e) => setIsSecret(e.target.checked)}
                />
                <span>Secret (write-only masked variable)</span>
              </label>

              <button
                type="submit"
                style={{
                  padding: '8px 16px',
                  backgroundColor: '#111827',
                  color: '#ffffff',
                  borderRadius: '6px',
                  border: 'none',
                  fontSize: '13px',
                  fontWeight: 500,
                  cursor: 'pointer',
                }}
              >
                Add Variable
              </button>
            </div>
          </form>
        </div>

        {/* Variables Table */}
        <div>
          <h2 style={{ margin: '0 0 16px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
            Configured Variables
          </h2>

          <div
            style={{
              backgroundColor: '#ffffff',
              border: '1px solid #e5e7eb',
              borderRadius: '8px',
              overflow: 'hidden',
              boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
            }}
          >
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13px', textAlign: 'left' }}>
              <thead>
                <tr style={{ backgroundColor: '#f9fafb', borderBottom: '1px solid #e5e7eb', color: '#6b7280' }}>
                  <th style={{ padding: '10px 16px', fontWeight: 600 }}>Key</th>
                  <th style={{ padding: '10px 16px', fontWeight: 600 }}>Value</th>
                  <th style={{ padding: '10px 16px', fontWeight: 600 }}>Type</th>
                  <th style={{ padding: '10px 16px', fontWeight: 600, textAlign: 'right' }}>Actions</th>
                </tr>
              </thead>
              <tbody>
                {variables.map((v) => (
                  <tr key={v.key} style={{ borderBottom: '1px solid #f3f4f6' }}>
                    <td style={{ padding: '12px 16px', fontWeight: 600, fontFamily: 'monospace', color: '#111827' }}>
                      {v.key}
                    </td>
                    <td style={{ padding: '12px 16px', fontFamily: 'monospace', color: '#4b5563' }}>
                      {v.isSecret ? '••••••••••••••••' : v.value}
                    </td>
                    <td style={{ padding: '12px 16px' }}>
                      {v.isSecret ? (
                        <span style={{ fontSize: '11px', padding: '2px 6px', backgroundColor: '#fef3c7', color: '#92400e', borderRadius: '4px' }}>
                          Secret
                        </span>
                      ) : (
                        <span style={{ fontSize: '11px', padding: '2px 6px', backgroundColor: '#f3f4f6', color: '#4b5563', borderRadius: '4px' }}>
                          Plaintext
                        </span>
                      )}
                    </td>
                    <td style={{ padding: '12px 16px', textAlign: 'right' }}>
                      <button
                        onClick={() => handleRemove(v.key)}
                        style={{
                          background: 'none',
                          border: 'none',
                          color: '#dc2626',
                          cursor: 'pointer',
                          fontSize: '12px',
                        }}
                      >
                        Remove
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </AppShell>
  );
}
