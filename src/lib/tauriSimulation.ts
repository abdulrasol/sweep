import { invoke as tauriInvoke } from '@tauri-apps/api/core';

// Hardened invoke bridge for Tauri v2
export const invoke = async <T = any>(cmd: string, args?: any): Promise<T> => {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (err) {
    console.error(`Tauri invoke error [${cmd}]:`, err);
    throw err;
  }
};

// This simulation hook is kept for backward compatibility with the Repo's UI components
// but can be replaced with real-time feedback from Rust in the future.
import { useState, useEffect } from 'react';
export const useScanningSimulation = (active: boolean) => {
  const [progress, setProgress] = useState(0);
  const [logs, setLogs] = useState<string[]>([]);
  const [freed, setFreed] = useState(0);

  useEffect(() => {
    if (!active) return;
    const interval = setInterval(() => {
      setProgress(prev => (prev >= 100 ? 100 : prev + 2));
      setFreed(prev => prev + 0.1);
    }, 100);
    return () => clearInterval(interval);
  }, [active]);

  return { progress, logs, freed };
};
