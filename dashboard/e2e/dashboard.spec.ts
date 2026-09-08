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
});
