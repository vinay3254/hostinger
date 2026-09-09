'use client';

import React, { useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api, MetricSeries, MetricPoint } from '../../../../lib/api';
import { AppShell } from '../../../../components/AppShell';
import { EmptyState } from '../../../../components/EmptyState';

const TIME_RANGES = [
  { label: '15m', value: '15m' },
  { label: '1h', value: '1h' },
  { label: '6h', value: '6h' },
  { label: '24h', value: '24h' },
  { label: '7d', value: '7d' },
];

const ENVIRONMENTS = [
  { label: 'All Environments', value: '' },
  { label: 'Production', value: 'production' },
  { label: 'Preview', value: 'preview' },
];

export default function ProjectMetricsPage() {
  const params = useParams();
  const queryClient = useQueryClient();
  const projectId = params.projectId as string;

  const [timeRange, setTimeRange] = useState('1h');
  const [environment, setEnvironment] = useState('');
  const [viewMode, setViewMode] = useState<'cards' | 'tables'>('cards');
  const [clearingCache, setClearingCache] = useState(false);
  const [cacheClearMessage, setCacheClearMessage] = useState<string | null>(null);

  const { data: project } = useQuery({
    queryKey: ['project', projectId],
    queryFn: () => api.getProject(projectId),
    enabled: Boolean(projectId),
  });

  const {
    data: metricsData,
    isLoading: metricsLoading,
    isError: metricsError,
    refetch: refetchMetrics,
  } = useQuery({
    queryKey: ['project-metrics', projectId, timeRange, environment],
    queryFn: () =>
      api.getProjectMetrics(projectId, {
        range: timeRange,
        environment: environment || undefined,
      }),
    enabled: Boolean(projectId),
    refetchInterval: 10000,
  });

  const clearCacheMutation = useMutation({
    mutationFn: () => api.clearProjectCache(projectId),
    onSuccess: (res) => {
      setCacheClearMessage(res.message || 'Cache cleared successfully');
      queryClient.invalidateQueries({ queryKey: ['project-metrics', projectId] });
      setTimeout(() => setCacheClearMessage(null), 4000);
    },
    onError: (err: any) => {
      setCacheClearMessage(`Failed to clear cache: ${err.message}`);
      setTimeout(() => setCacheClearMessage(null), 5000);
    },
  });

  const handleClearCache = async () => {
    if (
      typeof window !== 'undefined' &&
      !window.confirm('Are you sure you want to invalidate all cached build artifacts for this project?')
    ) {
      return;
    }
    setClearingCache(true);
    try {
      await clearCacheMutation.mutateAsync();
    } finally {
      setClearingCache(false);
    }
  };

  return (
    <AppShell currentProjectId={projectId}>
      {/* Breadcrumb */}
      <div style={{ fontSize: '13px', color: '#6b7280', marginBottom: '16px' }}>
        <Link href="/projects" style={{ color: '#2563eb', textDecoration: 'none' }}>
          Projects
        </Link>
        <span style={{ margin: '0 8px' }}>/</span>
        <Link href={`/projects/${projectId}/overview`} style={{ color: '#2563eb', textDecoration: 'none' }}>
          {project?.name || projectId}
        </Link>
        <span style={{ margin: '0 8px' }}>/</span>
        <span>Metrics</span>
      </div>

      {/* Header */}
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          flexWrap: 'wrap',
          gap: '16px',
          marginBottom: '24px',
        }}
      >
        <div>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: '#111827' }}>
            Telemetry & Metrics
          </h1>
          <p style={{ margin: '4px 0 0', fontSize: '14px', color: '#6b7280' }}>
            Real-time observability for build performance, HTTP requests, latencies, and resource consumption.
          </p>
        </div>

        <div style={{ display: 'flex', gap: '10px', alignItems: 'center' }}>
          <button
            onClick={handleClearCache}
            disabled={clearingCache}
            style={{
              padding: '8px 14px',
              borderRadius: '6px',
              border: '1px solid #d1d5db',
              backgroundColor: '#ffffff',
              color: '#374151',
              fontSize: '13px',
              fontWeight: 500,
              cursor: clearingCache ? 'not-allowed' : 'pointer',
            }}
          >
            {clearingCache ? 'Clearing Cache...' : 'Clear Build Cache'}
          </button>
        </div>
      </div>

      {cacheClearMessage && (
        <div
          role="status"
          style={{
            padding: '12px 16px',
            borderRadius: '6px',
            backgroundColor: '#ecfdf5',
            border: '1px solid #a7f3d0',
            color: '#065f46',
            fontSize: '13px',
            marginBottom: '20px',
          }}
        >
          {cacheClearMessage}
        </div>
      )}

      {/* Controls Bar */}
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '16px',
          backgroundColor: '#ffffff',
          padding: '14px 18px',
          borderRadius: '8px',
          border: '1px solid #e5e7eb',
          marginBottom: '24px',
        }}
      >
        {/* Time Range Selector */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span style={{ fontSize: '13px', fontWeight: 500, color: '#4b5563' }}>Range:</span>
          <div style={{ display: 'flex', borderRadius: '6px', border: '1px solid #d1d5db', overflow: 'hidden' }}>
            {TIME_RANGES.map((r) => (
              <button
                key={r.value}
                onClick={() => setTimeRange(r.value)}
                aria-pressed={timeRange === r.value}
                style={{
                  padding: '6px 12px',
                  fontSize: '12px',
                  fontWeight: timeRange === r.value ? 600 : 400,
                  backgroundColor: timeRange === r.value ? '#111827' : '#ffffff',
                  color: timeRange === r.value ? '#ffffff' : '#374151',
                  border: 'none',
                  cursor: 'pointer',
                  borderRight: '1px solid #e5e7eb',
                }}
              >
                {r.label}
              </button>
            ))}
          </div>
        </div>

        {/* Environment Filter */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <label htmlFor="env-select" style={{ fontSize: '13px', fontWeight: 500, color: '#4b5563' }}>
            Environment:
          </label>
          <select
            id="env-select"
            value={environment}
            onChange={(e) => setEnvironment(e.target.value)}
            style={{
              padding: '6px 10px',
              borderRadius: '6px',
              border: '1px solid #d1d5db',
              fontSize: '13px',
              backgroundColor: '#ffffff',
              color: '#374151',
              outline: 'none',
            }}
          >
            {ENVIRONMENTS.map((env) => (
              <option key={env.value} value={env.value}>
                {env.label}
              </option>
            ))}
          </select>
        </div>

        {/* View Mode & Metadata */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
          <div style={{ fontSize: '12px', color: '#6b7280' }}>
            <span>Timezone: <strong>UTC</strong></span>
            {metricsData?.last_updated && (
              <span style={{ marginLeft: '12px' }}>
                Updated: {new Date(metricsData.last_updated).toLocaleTimeString()}
              </span>
            )}
          </div>

          <button
            onClick={() => setViewMode(viewMode === 'cards' ? 'tables' : 'cards')}
            aria-label="Toggle accessible tabular view"
            style={{
              padding: '6px 12px',
              borderRadius: '6px',
              border: '1px solid #d1d5db',
              backgroundColor: viewMode === 'tables' ? '#f3f4f6' : '#ffffff',
              color: '#111827',
              fontSize: '12px',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {viewMode === 'cards' ? 'View as Tables' : 'View as Cards'}
          </button>
        </div>
      </div>

      {/* Metrics Content */}
      {metricsLoading ? (
        <div style={{ padding: '60px', textAlign: 'center', color: '#6b7280' }}>Loading metric series...</div>
      ) : metricsError ? (
        <div
          role="alert"
          style={{
            padding: '24px',
            borderRadius: '8px',
            backgroundColor: '#fef2f2',
            border: '1px solid #fecaca',
            color: '#991b1b',
          }}
        >
          Failed to load project metrics. Please verify API server connectivity.
        </div>
      ) : !metricsData || metricsData.series.length === 0 ? (
        <EmptyState
          title="No Metrics Configured"
          description="Metrics will appear once your deployment receives traffic or executes builds."
        />
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '24px' }}>
          {metricsData.series.map((series) => (
            <MetricCard
              key={series.metric_name}
              series={series}
              viewMode={viewMode}
              timeRange={timeRange}
            />
          ))}
        </div>
      )}
    </AppShell>
  );
}

interface MetricCardProps {
  series: MetricSeries;
  viewMode: 'cards' | 'tables';
  timeRange: string;
}

function formatMetricTitle(name: string): string {
  switch (name) {
    case 'request_total':
      return 'HTTP Requests';
    case 'request_error_total':
      return 'HTTP Error Rate';
    case 'request_latency_ms':
      return 'Response Latency';
    case 'container_cpu_seconds':
      return 'Container CPU Usage';
    case 'container_memory_bytes':
      return 'Container Memory';
    case 'build_duration_seconds':
      return 'Build Duration';
    case 'build_cache_hit_total':
      return 'Build Cache Hit Rate';
    case 'deployment_health_check_total':
      return 'Health Checks';
    default:
      return name;
  }
}

function formatValueWithUnit(val: number, unit: string): string {
  if (unit === 'bytes') {
    if (val >= 1024 * 1024 * 1024) {
      return `${(val / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    }
    return `${(val / (1024 * 1024)).toFixed(2)} MB`;
  }
  if (unit === 'milliseconds') {
    return `${val.toFixed(1)} ms`;
  }
  if (unit === 'seconds') {
    return `${val.toFixed(1)} s`;
  }
  return `${val.toLocaleString()} ${unit}`;
}

const MetricCard: React.FC<MetricCardProps> = ({ series, viewMode, timeRange }) => {
  const [showTable, setShowTable] = useState(false);
  const isTableView = viewMode === 'tables' || showTable;

  return (
    <div
      style={{
        backgroundColor: '#ffffff',
        borderRadius: '8px',
        border: '1px solid #e5e7eb',
        padding: '20px',
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
      }}
    >
      {/* Header */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '14px' }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <h3 style={{ margin: 0, fontSize: '16px', fontWeight: 600, color: '#111827' }}>
              {formatMetricTitle(series.metric_name)}
            </h3>
            <span
              style={{
                fontSize: '11px',
                fontWeight: 600,
                textTransform: 'uppercase',
                backgroundColor: '#f3f4f6',
                color: '#4b5563',
                padding: '2px 6px',
                borderRadius: '4px',
              }}
            >
              Unit: {series.unit}
            </span>
            {series.is_partial && (
              <span
                role="status"
                style={{
                  fontSize: '11px',
                  fontWeight: 500,
                  backgroundColor: '#fffbeb',
                  color: '#b45309',
                  border: '1px solid #fde68a',
                  padding: '2px 6px',
                  borderRadius: '4px',
                }}
              >
                ● Partial data: ongoing window
              </span>
            )}
          </div>
          <span style={{ fontSize: '12px', color: '#6b7280', marginTop: '4px', display: 'block' }}>
            Metric key: <code style={{ fontFamily: 'monospace' }}>{series.metric_name}</code> (Range: {timeRange})
          </span>
        </div>

        <button
          onClick={() => setShowTable(!showTable)}
          aria-label={`Toggle tabular data for ${series.metric_name}`}
          style={{
            fontSize: '12px',
            color: '#2563eb',
            background: 'transparent',
            border: 'none',
            cursor: 'pointer',
            padding: '4px 8px',
          }}
        >
          {isTableView ? 'Hide Table' : 'Show Table'}
        </button>
      </div>

      {/* Summary Stats Grid */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(130px, 1fr))',
          gap: '12px',
          padding: '12px 16px',
          backgroundColor: '#f9fafb',
          borderRadius: '6px',
          marginBottom: '16px',
        }}
      >
        <div>
          <span style={{ fontSize: '11px', color: '#6b7280', display: 'block' }}>Total / Count</span>
          <span style={{ fontSize: '15px', fontWeight: 600, color: '#111827' }}>
            {series.has_data ? series.summary.count.toLocaleString() : '0'} samples
          </span>
        </div>
        <div>
          <span style={{ fontSize: '11px', color: '#6b7280', display: 'block' }}>Average</span>
          <span style={{ fontSize: '15px', fontWeight: 600, color: '#111827' }}>
            {series.has_data ? formatValueWithUnit(series.summary.avg, series.unit) : '0'}
          </span>
        </div>
        <div>
          <span style={{ fontSize: '11px', color: '#6b7280', display: 'block' }}>Min / Max</span>
          <span style={{ fontSize: '14px', fontWeight: 500, color: '#111827' }}>
            {series.has_data
              ? `${formatValueWithUnit(series.summary.min, series.unit)} / ${formatValueWithUnit(series.summary.max, series.unit)}`
              : '0 / 0'}
          </span>
        </div>

        {series.summary.p50 != null && (
          <div>
            <span style={{ fontSize: '11px', color: '#6b7280', display: 'block' }}>P50 Latency</span>
            <span style={{ fontSize: '15px', fontWeight: 600, color: '#111827' }}>
              {formatValueWithUnit(series.summary.p50, series.unit)}
            </span>
          </div>
        )}
        {series.summary.p95 != null && (
          <div>
            <span style={{ fontSize: '11px', color: '#6b7280', display: 'block' }}>P95 Latency</span>
            <span style={{ fontSize: '15px', fontWeight: 600, color: '#111827' }}>
              {formatValueWithUnit(series.summary.p95, series.unit)}
            </span>
          </div>
        )}
        {series.summary.p99 != null && (
          <div>
            <span style={{ fontSize: '11px', color: '#6b7280', display: 'block' }}>P99 Latency</span>
            <span style={{ fontSize: '15px', fontWeight: 600, color: '#111827' }}>
              {formatValueWithUnit(series.summary.p99, series.unit)}
            </span>
          </div>
        )}
      </div>

      {/* Visual Chart / Representation */}
      {!isTableView && (
        <div>
          {!series.has_data ? (
            <div
              style={{
                padding: '28px',
                textAlign: 'center',
                color: '#6b7280',
                backgroundColor: '#f9fafb',
                borderRadius: '6px',
                fontSize: '13px',
              }}
            >
              No metric data collected for this project in the selected {timeRange} window.
            </div>
          ) : (
            <div
              style={{
                display: 'flex',
                alignItems: 'flex-end',
                gap: '4px',
                height: '100px',
                padding: '12px',
                backgroundColor: '#f9fafb',
                borderRadius: '6px',
                overflowX: 'auto',
              }}
            >
              {series.points.map((p, idx) => {
                const maxVal = Math.max(...series.points.map((pt) => pt.value), 1.0);
                const heightPct = Math.max(Math.min((p.value / maxVal) * 100, 100), 4);
                return (
                  <div
                    key={idx}
                    title={`Time: ${new Date(p.timestamp).toISOString()} | Value: ${p.value} (${p.count} samples)`}
                    style={{
                      flex: 1,
                      minWidth: '10px',
                      height: `${heightPct}%`,
                      backgroundColor: p.count > 0 ? '#3b82f6' : '#e5e7eb',
                      borderRadius: '2px 2px 0 0',
                      position: 'relative',
                    }}
                  />
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Accessible Table Alternative */}
      {isTableView && (
        <div style={{ overflowX: 'auto', marginTop: '12px' }}>
          <table
            aria-label={`Detailed tabular data for ${series.metric_name}`}
            style={{
              width: '100%',
              borderCollapse: 'collapse',
              fontSize: '12px',
              textAlign: 'left',
            }}
          >
            <thead>
              <tr style={{ borderBottom: '1px solid #e5e7eb', backgroundColor: '#f9fafb' }}>
                <th style={{ padding: '8px 12px', color: '#4b5563' }}>Timestamp (UTC)</th>
                <th style={{ padding: '8px 12px', color: '#4b5563' }}>Samples</th>
                <th style={{ padding: '8px 12px', color: '#4b5563' }}>Value ({series.unit})</th>
                <th style={{ padding: '8px 12px', color: '#4b5563' }}>Avg</th>
                <th style={{ padding: '8px 12px', color: '#4b5563' }}>Min / Max</th>
                {series.summary.p50 != null && (
                  <>
                    <th style={{ padding: '8px 12px', color: '#4b5563' }}>P50</th>
                    <th style={{ padding: '8px 12px', color: '#4b5563' }}>P95</th>
                    <th style={{ padding: '8px 12px', color: '#4b5563' }}>P99</th>
                  </>
                )}
                <th style={{ padding: '8px 12px', color: '#4b5563' }}>Status</th>
              </tr>
            </thead>
            <tbody>
              {series.points.length === 0 ? (
                <tr>
                  <td colSpan={8} style={{ padding: '16px', textAlign: 'center', color: '#6b7280' }}>
                    No points recorded.
                  </td>
                </tr>
              ) : (
                series.points.map((p, idx) => (
                  <tr key={idx} style={{ borderBottom: '1px solid #f3f4f6' }}>
                    <td style={{ padding: '6px 12px', fontFamily: 'monospace' }}>
                      {new Date(p.timestamp).toISOString()}
                    </td>
                    <td style={{ padding: '6px 12px' }}>{p.count}</td>
                    <td style={{ padding: '6px 12px', fontWeight: 600 }}>{p.value.toFixed(2)}</td>
                    <td style={{ padding: '6px 12px' }}>{p.avg.toFixed(2)}</td>
                    <td style={{ padding: '6px 12px' }}>
                      {p.min.toFixed(1)} / {p.max.toFixed(1)}
                    </td>
                    {series.summary.p50 != null && (
                      <>
                        <td style={{ padding: '6px 12px' }}>{p.p50 != null ? p.p50.toFixed(1) : '-'}</td>
                        <td style={{ padding: '6px 12px' }}>{p.p95 != null ? p.p95.toFixed(1) : '-'}</td>
                        <td style={{ padding: '6px 12px' }}>{p.p99 != null ? p.p99.toFixed(1) : '-'}</td>
                      </>
                    )}
                    <td style={{ padding: '6px 12px' }}>
                      {p.is_partial ? (
                        <span style={{ color: '#d97706', fontSize: '11px' }}>Partial</span>
                      ) : (
                        <span style={{ color: '#16a34a', fontSize: '11px' }}>Complete</span>
                      )}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
};
