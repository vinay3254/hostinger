'use client';

import React, { useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { api, RollbackResult } from '../lib/api';

interface RollbackDialogProps {
  isOpen: boolean;
  onClose: () => void;
  projectId: string;
  deploymentId: string;
  currentCommitSha?: string | null;
  environment?: string;
  onSuccess?: (result: RollbackResult) => void;
}

export const RollbackDialog: React.FC<RollbackDialogProps> = ({
  isOpen,
  onClose,
  projectId,
  deploymentId,
  currentCommitSha,
  environment = 'production',
  onSuccess,
}) => {
  const queryClient = useQueryClient();
  const [confirmed, setConfirmed] = useState(false);
  const [drainTimeoutSecs, setDrainTimeoutSecs] = useState<number>(5);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const rollbackMutation = useMutation({
    mutationFn: () =>
      api.rollbackDeployment(deploymentId, {
        drain_timeout_secs: drainTimeoutSecs,
      }),
    onSuccess: (data: RollbackResult) => {
      setErrorMessage(null);
      queryClient.invalidateQueries({ queryKey: ['deployment', deploymentId] });
      queryClient.invalidateQueries({ queryKey: ['deployments', projectId] });
      queryClient.invalidateQueries({ queryKey: ['releases', deploymentId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
      if (onSuccess) {
        onSuccess(data);
      }
      onClose();
    },
    onError: (err: Error) => {
      setErrorMessage(err.message || 'Rollback execution failed. Please check permissions and healthy predecessor availability.');
    },
  });

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="rollback-dialog-title"
    >
      <div className="w-full max-w-lg rounded-2xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
        <div className="flex items-center justify-between pb-4 border-b border-zinc-100 dark:border-zinc-800">
          <div className="flex items-center gap-2.5">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-amber-100 dark:bg-amber-950/60 text-amber-700 dark:text-amber-400 font-bold text-sm">
              !
            </span>
            <div>
              <h3
                id="rollback-dialog-title"
                className="text-base font-semibold text-zinc-900 dark:text-zinc-100"
              >
                Rollback Deployment
              </h3>
              <p className="text-xs text-zinc-500 dark:text-zinc-400">
                Restore previous healthy build artifact
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 p-1"
            disabled={rollbackMutation.isPending}
          >
            ✕
          </button>
        </div>

        <div className="py-4 space-y-4 text-sm text-zinc-600 dark:text-zinc-300">
          <div className="p-3.5 rounded-lg bg-amber-50/70 dark:bg-amber-950/20 border border-amber-200 dark:border-amber-900/40 text-xs text-amber-900 dark:text-amber-300 space-y-1">
            <p className="font-medium">Audited Zero-Downtime Rollback</p>
            <p className="text-amber-800/90 dark:text-amber-300/80">
              The platform will spin up the most recent healthy predecessor release, health-check it, switch traffic, and gracefully drain this deployment without downtime.
            </p>
          </div>

          <div className="grid grid-cols-2 gap-3 text-xs">
            <div className="p-3 rounded-lg border border-zinc-100 dark:border-zinc-800 bg-zinc-50/60 dark:bg-zinc-800/30">
              <span className="text-zinc-400 block mb-0.5">Environment</span>
              <span className="font-semibold text-zinc-800 dark:text-zinc-200 capitalize">
                {environment}
              </span>
            </div>
            <div className="p-3 rounded-lg border border-zinc-100 dark:border-zinc-800 bg-zinc-50/60 dark:bg-zinc-800/30">
              <span className="text-zinc-400 block mb-0.5">Active Commit</span>
              <span className="font-mono font-semibold text-zinc-800 dark:text-zinc-200">
                {currentCommitSha ? currentCommitSha.slice(0, 7) : 'Unassigned'}
              </span>
            </div>
          </div>

          <div>
            <label className="block text-xs font-medium text-zinc-700 dark:text-zinc-300 mb-1">
              Connection Drain Timeout
            </label>
            <select
              value={drainTimeoutSecs}
              onChange={(e) => setDrainTimeoutSecs(Number(e.target.value))}
              className="w-full text-xs rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 py-2 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
              disabled={rollbackMutation.isPending}
            >
              <option value={5}>5 seconds (Default fast drain)</option>
              <option value={15}>15 seconds (Standard API)</option>
              <option value={30}>30 seconds (Long-lived connections)</option>
            </select>
          </div>

          <label className="flex items-start gap-2.5 p-3 rounded-lg border border-zinc-200 dark:border-zinc-800 cursor-pointer hover:bg-zinc-50 dark:hover:bg-zinc-800/40 transition-colors">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(e) => setConfirmed(e.target.checked)}
              disabled={rollbackMutation.isPending}
              className="mt-0.5 h-4 w-4 rounded border-zinc-300 text-blue-600 focus:ring-blue-500"
            />
            <span className="text-xs text-zinc-700 dark:text-zinc-300 select-none">
              I authorize initiating a production traffic rollback. This action is durable and recorded in the audit log.
            </span>
          </label>

          {errorMessage && (
            <div className="p-3 rounded-lg bg-rose-50 dark:bg-rose-950/30 border border-rose-200 dark:border-rose-900 text-xs text-rose-800 dark:text-rose-300">
              <strong className="block font-semibold mb-0.5">Rollback Error:</strong>
              {errorMessage}
            </div>
          )}
        </div>

        <div className="flex items-center justify-end gap-3 pt-4 border-t border-zinc-100 dark:border-zinc-800">
          <button
            type="button"
            onClick={onClose}
            disabled={rollbackMutation.isPending}
            className="px-4 py-2 rounded-lg text-xs font-medium text-zinc-700 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => rollbackMutation.mutate()}
            disabled={!confirmed || rollbackMutation.isPending}
            className="px-4 py-2 rounded-lg text-xs font-medium bg-rose-600 hover:bg-rose-700 text-white shadow-sm disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {rollbackMutation.isPending ? (
              <>
                <span className="w-3 h-3 border-2 border-white/40 border-t-white rounded-full animate-spin" />
                Executing Rollback...
              </>
            ) : (
              'Confirm Rollback'
            )}
          </button>
        </div>
      </div>
    </div>
  );
};
