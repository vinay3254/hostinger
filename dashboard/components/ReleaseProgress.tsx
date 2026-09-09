'use client';

import React, { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, Release, ReleaseStatus, ReleaseTransitionRecord } from '../lib/api';

interface ReleaseProgressProps {
  deploymentId: string;
}

const STATUS_STEPS: { key: ReleaseStatus; label: string; description: string }[] = [
  { key: 'starting', label: 'Start', description: 'Container allocated' },
  { key: 'health_checking', label: 'Health Probe', description: 'Gating pre-traffic checks' },
  { key: 'ready', label: 'Ready', description: 'Health check passed' },
  { key: 'active', label: 'Active Traffic', description: 'Serving live traffic' },
  { key: 'draining', label: 'Draining', description: 'Gracefully closing connections' },
  { key: 'stopped', label: 'Stopped', description: 'Cleanly retired' },
];

function getStepIndex(status: ReleaseStatus): number {
  switch (status) {
    case 'starting':
      return 0;
    case 'health_checking':
      return 1;
    case 'ready':
      return 2;
    case 'active':
      return 3;
    case 'draining':
      return 4;
    case 'stopped':
      return 5;
    case 'failed':
      return -1;
    default:
      return 0;
  }
}

export const ReleaseProgress: React.FC<ReleaseProgressProps> = ({ deploymentId }) => {
  const [expandedReleaseId, setExpandedReleaseId] = useState<string | null>(null);

  const { data: releases, isLoading, error } = useQuery({
    queryKey: ['releases', deploymentId],
    queryFn: () => api.listDeploymentReleases(deploymentId),
    refetchInterval: (query) => {
      const data = query.state.data;
      if (
        data &&
        data.some((r) =>
          ['starting', 'health_checking', 'draining'].includes(r.status)
        )
      ) {
        return 1000;
      }
      return 5000;
    },
  });

  const { data: events, isLoading: eventsLoading } = useQuery({
    queryKey: ['release-events', expandedReleaseId],
    queryFn: () => (expandedReleaseId ? api.listReleaseEvents(expandedReleaseId) : Promise.resolve([])),
    enabled: Boolean(expandedReleaseId),
  });

  if (isLoading) {
    return (
      <div className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-6 shadow-sm animate-pulse">
        <div className="h-5 w-44 bg-zinc-200 dark:bg-zinc-800 rounded mb-4" />
        <div className="h-10 w-full bg-zinc-100 dark:bg-zinc-800/60 rounded-lg" />
      </div>
    );
  }

  if (error || !releases || releases.length === 0) {
    return null;
  }

  return (
    <div className="rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-6 shadow-sm">
      <div className="flex flex-wrap items-center justify-between gap-4 mb-6">
        <div>
          <h3 className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
            Zero-Downtime Release Pipeline
          </h3>
          <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">
            Health-gated traffic router with graceful drainage and crash recovery.
          </p>
        </div>
        <span className="inline-flex items-center px-2.5 py-1 rounded-full text-xs font-medium bg-emerald-50 dark:bg-emerald-950/40 text-emerald-700 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse mr-1.5" />
          Active Route Protected
        </span>
      </div>

      <div className="space-y-6">
        {releases.map((release: Release) => {
          const currentIndex = getStepIndex(release.status);
          const isFailed = release.status === 'failed';
          const isExpanded = expandedReleaseId === release.id;

          return (
            <div
              key={release.id}
              className="rounded-lg border border-zinc-100 dark:border-zinc-800/80 bg-zinc-50/50 dark:bg-zinc-900/40 p-5"
            >
              <div className="flex flex-wrap items-center justify-between gap-3 mb-4">
                <div className="flex items-center gap-2.5">
                  <span className="font-mono text-xs font-semibold px-2 py-0.5 rounded bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200">
                    {`Release v${release.version}`}
                  </span>
                  <span className="text-xs font-mono text-zinc-400 dark:text-zinc-500">
                    {release.id.slice(0, 8)}
                  </span>
                  {release.status === 'active' && (
                    <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-emerald-100 dark:bg-emerald-900/50 text-emerald-800 dark:text-emerald-300">
                      Live Traffic
                    </span>
                  )}
                  {release.status === 'draining' && (
                    <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-amber-100 dark:bg-amber-900/50 text-amber-800 dark:text-amber-300">
                      Draining Connections
                    </span>
                  )}
                  {isFailed && (
                    <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-rose-100 dark:bg-rose-900/50 text-rose-800 dark:text-rose-300">
                      Health Check Failed
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-3 text-xs text-zinc-500 dark:text-zinc-400">
                  {release.port && (
                    <span>
                      Port: <strong className="font-mono">{release.port}</strong>
                    </span>
                  )}
                  <button
                    onClick={() => setExpandedReleaseId(isExpanded ? null : release.id)}
                    className="text-xs text-blue-600 dark:text-blue-400 hover:underline font-medium"
                  >
                    {isExpanded ? 'Hide Events' : 'View Events'}
                  </button>
                </div>
              </div>

              {/* Lifecycle Step Tracker */}
              <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-2">
                {STATUS_STEPS.map((step, idx) => {
                  let stepState: 'complete' | 'current' | 'upcoming' | 'failed' = 'upcoming';

                  if (isFailed) {
                    stepState = idx === 1 ? 'failed' : idx === 0 ? 'complete' : 'upcoming';
                  } else if (idx < currentIndex) {
                    stepState = 'complete';
                  } else if (idx === currentIndex) {
                    stepState = 'current';
                  }

                  return (
                    <div
                      key={step.key}
                      className={`p-3 rounded-lg border text-left transition-all ${
                        stepState === 'current'
                          ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-950/20 dark:border-blue-800'
                          : stepState === 'complete'
                          ? 'border-emerald-200 dark:border-emerald-900/40 bg-emerald-50/30 dark:bg-emerald-950/10'
                          : stepState === 'failed'
                          ? 'border-rose-300 dark:border-rose-900 bg-rose-50/40 dark:bg-rose-950/20'
                          : 'border-zinc-200 dark:border-zinc-800 bg-white/60 dark:bg-zinc-900/20 opacity-60'
                      }`}
                    >
                      <div className="flex items-center gap-1.5 mb-1">
                        <span
                          className={`w-2 h-2 rounded-full ${
                            stepState === 'current'
                              ? 'bg-blue-500 animate-pulse'
                              : stepState === 'complete'
                              ? 'bg-emerald-500'
                              : stepState === 'failed'
                              ? 'bg-rose-500'
                              : 'bg-zinc-300 dark:bg-zinc-700'
                          }`}
                        />
                        <span className="text-xs font-semibold text-zinc-900 dark:text-zinc-100">
                          {step.label}
                        </span>
                      </div>
                      <p className="text-[11px] text-zinc-500 dark:text-zinc-400 line-clamp-1">
                        {step.description}
                      </p>
                    </div>
                  );
                })}
              </div>

              {/* Expandable Transition Log */}
              {isExpanded && (
                <div className="mt-4 pt-4 border-t border-zinc-200 dark:border-zinc-800">
                  <h4 className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 mb-2">
                    Durable Transition Audit Trail
                  </h4>
                  {eventsLoading ? (
                    <div className="text-xs text-zinc-400 animate-pulse">Loading transition log...</div>
                  ) : !events || events.length === 0 ? (
                    <div className="text-xs text-zinc-400">No transition records recorded yet.</div>
                  ) : (
                    <div className="space-y-1.5">
                      {events.map((evt: ReleaseTransitionRecord) => (
                        <div
                          key={evt.id}
                          className="flex items-center justify-between text-xs py-1 px-2 rounded bg-zinc-100/70 dark:bg-zinc-800/40 font-mono"
                        >
                          <div className="flex items-center gap-2">
                            <span className="text-zinc-500 dark:text-zinc-400">{evt.from_status}</span>
                            <span className="text-zinc-400">→</span>
                            <span className="font-semibold text-zinc-800 dark:text-zinc-200">
                              {evt.to_status}
                            </span>
                            {evt.reason && (
                              <span className="text-[11px] text-zinc-500 font-sans italic">
                                ({evt.reason})
                              </span>
                            )}
                          </div>
                          <span className="text-[11px] text-zinc-400 font-sans">
                            {new Date(evt.created_at).toLocaleTimeString()}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};
