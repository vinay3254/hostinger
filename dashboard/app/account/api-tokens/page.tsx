'use client';

import React, { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api, IssuedToken } from '../../../lib/api';
import { AppShell } from '../../../components/AppShell';
import { EmptyState } from '../../../components/EmptyState';

export default function ApiTokensPage() {
  const queryClient = useQueryClient();
  const [tokenName, setTokenName] = useState('');
  const [scopes, setScopes] = useState<string[]>(['*']);
  const [issuedToken, setIssuedToken] = useState<IssuedToken | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const { data: tokens = [], isLoading } = useQuery({
    queryKey: ['api-tokens'],
    queryFn: () => api.listApiTokens(),
  });

  const createMutation = useMutation({
    mutationFn: (input: { name: string; scopes: string[] }) => api.createApiToken(input),
    onSuccess: (token) => {
      setIssuedToken(token);
      setTokenName('');
      queryClient.invalidateQueries({ queryKey: ['api-tokens'] });
    },
  });

  const revokeMutation = useMutation({
    mutationFn: (id: string) => api.revokeApiToken(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['api-tokens'] });
    },
  });

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!tokenName.trim()) return;
    setError(null);
    setCreating(true);

    try {
      await createMutation.mutateAsync({
        name: tokenName.trim(),
        scopes,
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to generate token');
    } finally {
      setCreating(false);
    }
  };

  const handleRevoke = async (id: string, name: string) => {
    if (confirm(`Are you sure you want to revoke API token "${name}"? This action cannot be undone.`)) {
      await revokeMutation.mutateAsync(id);
    }
  };

  const handleCopy = () => {
    if (issuedToken && typeof navigator !== 'undefined') {
      navigator.clipboard.writeText(issuedToken.raw_token);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  return (
    <AppShell>
      <div style={{ maxWidth: '800px', margin: '0 auto' }}>
        <div style={{ marginBottom: '28px' }}>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Personal API Tokens
          </h1>
          <p style={{ margin: '4px 0 0 0', fontSize: '14px', color: '#6b7280' }}>
            Authenticate CLI operations and automated scripts without session cookies
          </p>
        </div>

        {/* One-time Token Display Alert */}
        {issuedToken && (
          <div
            role="alert"
            style={{
              marginBottom: '28px',
              padding: '20px',
              borderRadius: '8px',
              backgroundColor: '#ecfdf5',
              border: '1px solid #a7f3d0',
              color: '#065f46',
            }}
          >
            <h3 style={{ margin: '0 0 8px 0', fontSize: '15px', fontWeight: 600 }}>
              Token Created Successfully!
            </h3>
            <p style={{ margin: '0 0 12px 0', fontSize: '13px', color: '#047857' }}>
              Please copy your personal API token now. <strong>You will not be able to see it again!</strong>
            </p>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: '12px',
                backgroundColor: '#ffffff',
                padding: '10px 14px',
                borderRadius: '6px',
                border: '1px solid #6ee7b7',
              }}
            >
              <code
                style={{
                  fontFamily: 'monospace',
                  fontSize: '13px',
                  fontWeight: 600,
                  color: '#111827',
                  flex: 1,
                  wordBreak: 'break-all',
                }}
              >
                {issuedToken.raw_token}
              </code>
              <button
                onClick={handleCopy}
                style={{
                  padding: '6px 12px',
                  backgroundColor: '#065f46',
                  color: '#ffffff',
                  borderRadius: '4px',
                  border: 'none',
                  fontSize: '12px',
                  fontWeight: 500,
                  cursor: 'pointer',
                }}
              >
                {copied ? 'Copied!' : 'Copy'}
              </button>
            </div>
          </div>
        )}

        {/* Generate Token Form */}
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
            Generate New Token
          </h2>

          {error && (
            <div
              style={{
                marginBottom: '16px',
                padding: '10px 14px',
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

          <form onSubmit={handleCreate} style={{ display: 'flex', gap: '12px', alignItems: 'flex-end' }}>
            <div style={{ flex: 1 }}>
              <label
                htmlFor="token-name"
                style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
              >
                Token Description / Name
              </label>
              <input
                id="token-name"
                type="text"
                required
                placeholder="e.g. laptop-cli, github-action"
                value={tokenName}
                onChange={(e) => setTokenName(e.target.value)}
                style={{
                  width: '100%',
                  padding: '8px 12px',
                  borderRadius: '6px',
                  border: '1px solid #d1d5db',
                  fontSize: '14px',
                  boxSizing: 'border-box',
                }}
              />
            </div>

            <button
              type="submit"
              disabled={creating || !tokenName.trim()}
              style={{
                padding: '9px 18px',
                backgroundColor: '#111827',
                color: '#ffffff',
                borderRadius: '6px',
                border: 'none',
                fontSize: '14px',
                fontWeight: 500,
                cursor: creating ? 'not-allowed' : 'pointer',
                opacity: creating ? 0.7 : 1,
              }}
            >
              {creating ? 'Generating...' : 'Generate Token'}
            </button>
          </form>
        </div>

        {/* Token List */}
        <div>
          <h2 style={{ margin: '0 0 16px 0', fontSize: '16px', fontWeight: 600, color: '#111827' }}>
            Active API Tokens
          </h2>

          {isLoading ? (
            <div style={{ padding: '32px', textAlign: 'center', color: '#6b7280' }}>Loading tokens...</div>
          ) : tokens.length === 0 ? (
            <EmptyState
              title="No API tokens created"
              description="Generate a personal API token to authenticate CLI and programmatic deploys."
            />
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
              {tokens.map((token) => (
                <div
                  key={token.id}
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    padding: '14px 18px',
                    backgroundColor: '#ffffff',
                    border: '1px solid #e5e7eb',
                    borderRadius: '8px',
                    opacity: token.revoked ? 0.6 : 1,
                  }}
                >
                  <div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                      <span style={{ fontWeight: 600, fontSize: '14px', color: '#111827' }}>
                        {token.name}
                      </span>
                      <code
                        style={{
                          fontSize: '12px',
                          color: '#6b7280',
                          backgroundColor: '#f3f4f6',
                          padding: '2px 6px',
                          borderRadius: '4px',
                        }}
                      >
                        {token.prefix}...
                      </code>
                      {token.revoked && (
                        <span
                          style={{
                            fontSize: '11px',
                            color: '#991b1b',
                            backgroundColor: '#fef2f2',
                            padding: '2px 6px',
                            borderRadius: '9999px',
                          }}
                        >
                          Revoked
                        </span>
                      )}
                    </div>
                    <div style={{ fontSize: '12px', color: '#6b7280', marginTop: '4px' }}>
                      Created {new Date(token.created_at).toLocaleDateString()}
                      {token.last_used_at && ` • Last used ${new Date(token.last_used_at).toLocaleDateString()}`}
                    </div>
                  </div>

                  {!token.revoked && (
                    <button
                      onClick={() => handleRevoke(token.id, token.name)}
                      style={{
                        padding: '6px 12px',
                        borderRadius: '6px',
                        border: '1px solid #fecaca',
                        backgroundColor: '#ffffff',
                        color: '#dc2626',
                        fontSize: '12px',
                        fontWeight: 500,
                        cursor: 'pointer',
                      }}
                    >
                      Revoke
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </AppShell>
  );
}
