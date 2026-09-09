import { test, expect } from '@playwright/test';

test.describe('Dashboard E2E Workflow with Mocked API', () => {
  test.beforeEach(async ({ page }) => {
    // Mock /v1/me
    await page.route('**/v1/me', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'u-1',
          email: 'user@example.com',
          name: 'Test User',
          scopes: ['*'],
        }),
      });
    });

    // Mock /v1/auth/session
    await page.route('**/v1/auth/session', async (route) => {
      if (route.request().method() === 'POST') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            user: { id: 'u-1', email: 'user@example.com', name: 'Test User' },
            token: 'mock-token',
          }),
        });
      } else {
        await route.fulfill({ status: 204 });
      }
    });

    // Mock /v1/projects
    await page.route('**/v1/projects', async (route) => {
      if (route.request().method() === 'POST') {
        const body = JSON.parse(route.request().postData() || '{}');
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            id: 'proj-1234',
            name: body.name || 'e2e-project',
            source_dir: body.source_dir || '/tmp/site',
            base_image: body.base_image || '/tmp/base.tar.gz',
            server_command: ['/bin/busybox', 'httpd', '-f', '-p', '{PORT}', '-h', '/srv/app'],
            created_at: new Date().toISOString(),
            active_deployment: null,
          }),
        });
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify([
            {
              id: 'proj-1234',
              name: 'e2e-project',
              source_dir: '/tmp/site',
              base_image: '/tmp/base.tar.gz',
              server_command: ['/bin/busybox', 'httpd', '-f', '-p', '{PORT}', '-h', '/srv/app'],
              created_at: new Date().toISOString(),
              active_deployment: 'dep-1234',
            },
          ]),
        });
      }
    });

    // Mock /v1/projects/proj-1234
    await page.route('**/v1/projects/proj-1234', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'proj-1234',
          name: 'e2e-project',
          source_dir: '/tmp/site',
          base_image: '/tmp/base.tar.gz',
          server_command: ['/bin/busybox', 'httpd', '-f', '-p', '{PORT}', '-h', '/srv/app'],
          created_at: new Date().toISOString(),
          active_deployment: 'dep-1234',
        }),
      });
    });

    // Mock /v1/projects/proj-1234/deployments
    await page.route('**/v1/projects/proj-1234/deployments', async (route) => {
      if (route.request().method() === 'POST') {
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            id: 'dep-1234',
            project_id: 'proj-1234',
            framework: 'static',
            status: 'running',
            port: 43123,
            url: 'http://127.0.0.1:43123',
            created_at: new Date().toISOString(),
          }),
        });
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify([
            {
              id: 'dep-1234',
              project_id: 'proj-1234',
              framework: 'static',
              status: 'running',
              port: 43123,
              url: 'http://127.0.0.1:43123',
              created_at: new Date().toISOString(),
            },
          ]),
        });
      }
    });

    // Mock /v1/deployments/dep-1234
    await page.route('**/v1/deployments/dep-1234', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'dep-1234',
          project_id: 'proj-1234',
          framework: 'static',
          status: 'running',
          port: 43123,
          url: 'http://127.0.0.1:43123',
          created_at: new Date().toISOString(),
        }),
      });
    });

    // Mock /v1/deployments/dep-1234/logs
    await page.route('**/v1/deployments/dep-1234/logs', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          logs: 'Step 1: Extracting static assets\nStep 2: Starting server on :43123\nReady and serving HTTP',
        }),
      });
    });

    // Mock /v1/deployments/dep-1234/releases
    await page.route('**/v1/deployments/dep-1234/releases', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            id: 'rel-1',
            deployment_id: 'dep-1234',
            version: 1,
            container_id: 'c-1234',
            port: 43123,
            status: 'active',
            is_active_route: true,
            health_checked_at: new Date().toISOString(),
            activated_at: new Date().toISOString(),
            created_at: new Date().toISOString(),
          },
        ]),
      });
    });

    // Mock /v1/releases/rel-1/events
    await page.route('**/v1/releases/rel-1/events', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            id: 'evt-1',
            release_id: 'rel-1',
            from_status: 'ready',
            to_status: 'active',
            reason: 'Traffic cutover complete',
            created_at: new Date().toISOString(),
          },
        ]),
      });
    });

    // Mock /v1/deployments/dep-1234/rollback
    await page.route('**/v1/deployments/dep-1234/rollback', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          status: 'rolled_back',
          rollback_deployment_id: 'dep-rb-5678',
          target_release_id: 'rel-1',
          previous_healthy_deployment_id: 'dep-1234',
          active_route_url: 'http://127.0.0.1:43123',
        }),
      });
    });

    // Mock /v1/providers/github/repositories
    await page.route('**/v1/providers/github/repositories', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            id: 'repo-e2e',
            connection_id: 'conn-1',
            external_id: '12345',
            full_name: 'test-org/test-repo',
            clone_url: 'https://github.com/test-org/test-repo.git',
            default_branch: 'main',
            synced_at: new Date().toISOString(),
          },
        ]),
      });
    });

    // Mock /v1/projects/proj-1234/source
    let projectSource: any = {
      repository: null,
      provider: null,
      target_branch: 'main',
      webhook_url: null,
      has_webhook_secret: false,
      last_delivery_at: null,
    };

    await page.route('**/v1/projects/proj-1234/source', async (route) => {
      if (route.request().method() === 'POST') {
        const body = JSON.parse(route.request().postData() || '{}');
        projectSource = {
          repository: {
            id: body.repository_id || 'repo-e2e',
            connection_id: 'conn-1',
            external_id: '12345',
            full_name: 'test-org/test-repo',
            clone_url: 'https://github.com/test-org/test-repo.git',
            default_branch: 'main',
            synced_at: new Date().toISOString(),
          },
          provider: 'github',
          target_branch: body.target_branch || 'main',
          webhook_url: '/v1/webhooks/github/proj-1234',
          has_webhook_secret: true,
          last_delivery_at: null,
        };
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(projectSource),
        });
      } else if (route.request().method() === 'DELETE') {
        projectSource = {
          repository: null,
          provider: null,
          target_branch: 'main',
          webhook_url: null,
          has_webhook_secret: false,
          last_delivery_at: null,
        };
        await route.fulfill({ status: 204 });
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(projectSource),
        });
      }
    });

    // Mock /v1/projects/proj-1234/source/events
    await page.route('**/v1/projects/proj-1234/source/events', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            id: 'ev-1',
            project_id: 'proj-1234',
            provider: 'github',
            delivery_id: 'del-12345678',
            kind: 'push',
            commit_sha: 'a1b2c3d4e5f6',
            branch: 'main',
            pull_request: null,
            idempotency_key: 'github:del-12345678:a1b2c3d4e5f6',
            created_at: new Date().toISOString(),
          },
        ]),
      });
    });

    // Mock /v1/projects/proj-1234/previews
    await page.route('**/v1/projects/proj-1234/previews', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            id: 'prev-1234',
            project_id: 'proj-1234',
            provider: 'github',
            pr_number: 42,
            head_sha: 'a1b2c3d4e5f6',
            base_branch: 'main',
            head_branch: 'feature/e2e',
            deployment_id: 'dep-1234',
            hostname: 'e2e-project-pr-42.preview.local',
            status: 'ready',
            closed_at: null,
            cleanup_attempt: 0,
            created_at: new Date().toISOString(),
            updated_at: new Date().toISOString(),
          },
        ]),
      });
    });

    // Mock /v1/previews/prev-1234
    await page.route('**/v1/previews/prev-1234', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'prev-1234',
          project_id: 'proj-1234',
          provider: 'github',
          pr_number: 42,
          head_sha: 'a1b2c3d4e5f6',
          base_branch: 'main',
          head_branch: 'feature/e2e',
          deployment_id: 'dep-1234',
          hostname: 'e2e-project-pr-42.preview.local',
          status: 'ready',
          closed_at: null,
          cleanup_attempt: 0,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        }),
      });
    });

    // Mock /v1/previews/prev-1234/promote
    await page.route('**/v1/previews/prev-1234/promote', async (route) => {
      await route.fulfill({
        status: 202,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'dep-prod-999',
          project_id: 'proj-1234',
          framework: 'static',
          status: 'queued',
          created_at: new Date().toISOString(),
          commit_sha: 'a1b2c3d4e5f6',
          target: 'production',
        }),
      });
    });

    // Mock /v1/projects/proj-1234/metrics
    await page.route('**/v1/projects/proj-1234/metrics*', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          project_id: 'proj-1234',
          range: '1h',
          resolution: '1m',
          start: new Date(Date.now() - 3600000).toISOString(),
          end: new Date().toISOString(),
          last_updated: new Date().toISOString(),
          series: [
            {
              metric_name: 'request_total',
              unit: 'count',
              environment: 'production',
              points: [
                {
                  timestamp: new Date(Date.now() - 60000).toISOString(),
                  count: 42,
                  value: 42,
                  sum: 42,
                  min: 42,
                  max: 42,
                  avg: 42,
                  is_partial: true,
                },
              ],
              summary: {
                total: 42,
                count: 42,
                avg: 42,
                min: 42,
                max: 42,
              },
              has_data: true,
              is_partial: true,
            },
          ],
        }),
      });
    });

    // Mock /v1/projects/proj-1234/cache/clear
    await page.route('**/v1/projects/proj-1234/cache/clear', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          message: 'Project build cache cleared successfully',
        }),
      });
    });
  });

  test('user logs in and views dashboard', async ({ page }) => {
    await page.goto('/login');
    await page.fill('#email', 'user@example.com');
    await page.fill('#password', 'SuperPassword123!');
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL(/.*dashboard/);
    await expect(page.getByRole('heading', { name: 'e2e-project' })).toBeVisible();
  });

  test('user creates a new project', async ({ page }) => {
    await page.goto('/projects/new');
    await page.fill('#project-name', 'new-site');
    await page.fill('#source-dir', '/tmp/my-site');
    await page.fill('#base-image', '/tmp/base.tar.gz');
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL(/.*projects\/proj-1234\/overview/);
    await expect(page.getByRole('heading', { name: 'e2e-project' })).toBeVisible();
  });

  test('user views deployment detail and logs', async ({ page }) => {
    await page.goto('/projects/proj-1234/deployments/dep-1234');
    await expect(page.getByRole('heading', { name: 'dep-1234' })).toBeVisible();
    await expect(page.getByText('http://127.0.0.1:43123')).toBeVisible();
    await expect(page.getByText(/Ready and serving HTTP/)).toBeVisible();
  });

  test('user configures git source and views webhook settings', async ({ page }) => {
    await page.goto('/projects/proj-1234/settings/source');
    await expect(page.getByRole('heading', { name: 'Source Settings' })).toBeVisible();

    // Link repository
    await page.click('button:has-text("Link Repository")');
    await expect(page.getByText('test-org/test-repo')).toBeVisible();
    await expect(page.getByText('Active (Configured)')).toBeVisible();
  });

  test('user views previews and inspects preview details and promotes', async ({ page }) => {
    await page.goto('/projects/proj-1234/previews');
    await expect(page.getByRole('heading', { name: 'Pull Request Previews' })).toBeVisible();
    await expect(page.getByText('PR #42')).toBeVisible();
    await expect(page.getByText('e2e-project-pr-42.preview.local')).toBeVisible();

    // Click details
    await page.click('a:has-text("Details")');
    await expect(page.getByRole('heading', { name: 'Preview: PR #42' })).toBeVisible();
    await expect(page.getByText(/Open Preview/i)).toBeVisible();

    // Promote to production
    await page.click('button:has-text("Promote to Production")');
    await expect(page.getByText(/Preview promoted to production!/i)).toBeVisible();
  });

  test('user views project metrics and toggles tabular view and clears cache', async ({ page }) => {
    // Handle browser confirm dialog
    page.on('dialog', async (dialog) => {
      await dialog.accept();
    });

    await page.goto('/projects/proj-1234/metrics');
    await expect(page.getByRole('heading', { name: 'Telemetry & Metrics' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'HTTP Requests' })).toBeVisible();

    // Toggle table view
    await page.click('button:has-text("View as Tables")');
    await expect(page.getByRole('table', { name: /Detailed tabular data for request_total/i })).toBeVisible();

    // Clear cache
    await page.click('button:has-text("Clear Build Cache")');
    await expect(page.getByText(/Project build cache cleared successfully/i)).toBeVisible();
  });

  test('user views release pipeline progress and initiates rollback', async ({ page }) => {
    await page.goto('/projects/proj-1234/deployments/dep-1234');
    await expect(page.getByText('Zero-Downtime Release Pipeline')).toBeVisible();
    await expect(page.getByText('Active Route Protected')).toBeVisible();
    await expect(page.getByText('Live Traffic', { exact: true })).toBeVisible();

    // Toggle release event audit trail
    await page.click('button:has-text("View Events")');
    await expect(page.getByText(/Traffic cutover complete/i)).toBeVisible();

    // Open rollback dialog
    await page.click('button[data-testid="rollback-button"]');
    await expect(page.getByRole('heading', { name: 'Rollback Deployment' })).toBeVisible();

    // Confirm checkbox and execute
    await page.click('input[type="checkbox"]');
    await page.click('button:has-text("Confirm Rollback")');

    // Dialog closes
    await expect(page.getByRole('heading', { name: 'Rollback Deployment' })).not.toBeVisible();
  });
});

