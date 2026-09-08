import { describe, it, expect, vi } from 'vitest';
import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';

describe('Environment Variable Key Validation', () => {
  const isValidEnvKey = (key: string) => /^[A-Z_][A-Z0-9_]*$/.test(key);

  it('accepts valid uppercase environment variable keys', () => {
    expect(isValidEnvKey('PORT')).toBe(true);
    expect(isValidEnvKey('DATABASE_URL')).toBe(true);
    expect(isValidEnvKey('NEXT_PUBLIC_API_URL')).toBe(true);
  });

  it('rejects invalid environment variable keys', () => {
    expect(isValidEnvKey('123_KEY')).toBe(false);
    expect(isValidEnvKey('api-key')).toBe(false);
    expect(isValidEnvKey('KEY WITH SPACE')).toBe(false);
    expect(isValidEnvKey('')).toBe(false);
  });
});

describe('Token One-Time Display', () => {
  it('displays raw token once with clear warning to save it', () => {
    const rawToken = 'dp_a1b2c3d4e5f6';
    const TokenDisplay = ({ token }: { token: string | null }) => (
      <div>
        {token ? (
          <div role="alert" data-testid="token-box">
            <p>Save this token now! You will not be able to see it again.</p>
            <code>{token}</code>
          </div>
        ) : (
          <p>No active token display</p>
        )}
      </div>
    );

    const { rerender } = render(<TokenDisplay token={rawToken} />);
    expect(screen.getByTestId('token-box')).toBeDefined();
    expect(screen.getByText(rawToken)).toBeDefined();
    expect(screen.getByText(/You will not be able to see it again/i)).toBeDefined();

    // After dismiss / subsequent render, raw token is absent
    rerender(<TokenDisplay token={null} />);
    expect(screen.queryByTestId('token-box')).toBeNull();
  });
});

describe('Write-Only Secret Rendering', () => {
  it('masks secret values and never renders raw secrets to DOM', () => {
    const secretValue = 'super_secret_stripe_api_key_12345';
    const EnvRow = ({ name, value, isSecret }: { name: string; value: string; isSecret: boolean }) => (
      <tr data-testid={`env-${name}`}>
        <td>{name}</td>
        <td>{isSecret ? '••••••••••••••••' : value}</td>
      </tr>
    );

    render(
      <table>
        <tbody>
          <EnvRow name="STRIPE_SECRET" value={secretValue} isSecret={true} />
          <EnvRow name="PORT" value="8080" isSecret={false} />
        </tbody>
      </table>
    );

    expect(screen.getByText('STRIPE_SECRET')).toBeDefined();
    expect(screen.getByText('••••••••••••••••')).toBeDefined();
    expect(screen.queryByText(secretValue)).toBeNull();
    expect(screen.getByText('8080')).toBeDefined();
  });
});

describe('Token Revocation Confirmation', () => {
  it('triggers confirmation before invoking revoke callback', () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    const onRevoke = vi.fn();

    const RevokeButton = ({ tokenId, name }: { tokenId: string; name: string }) => (
      <button
        onClick={() => {
          if (window.confirm(`Revoke ${name}?`)) {
            onRevoke(tokenId);
          }
        }}
      >
        Revoke
      </button>
    );

    render(<RevokeButton tokenId="tok-1" name="my-token" />);
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));

    expect(confirmSpy).toHaveBeenCalledWith('Revoke my-token?');
    expect(onRevoke).toHaveBeenCalledWith('tok-1');

    confirmSpy.mockRestore();
  });
});

import { redactParams } from '../lib/api';

describe('API Client Redaction', () => {
  it('sanitizes and redacts secrets from query cache keys and telemetry', () => {
    const params = {
      user: 'vinay',
      token: 'dp_super_secret_val',
      api_key: 'stripe_live_123',
    };

    const redacted = redactParams(params);
    expect(redacted.token).toBe('[REDACTED]');
    expect(redacted.api_key).toBe('[REDACTED]');
    expect(redacted.user).toBe('vinay');
  });
});
