import { invoke as tauriInvoke } from '@tauri-apps/api/core';

/** Typed bridge to Sweep's Rust commands. */
export const invoke = async <T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (err) {
    console.error(`Tauri invoke error [${cmd}]:`, err);
    throw err;
  }
};

/** Format a byte count for display. The engine always sends raw bytes. */
export const formatBytes = (bytes: number): string => {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
};

/** Open an https link in the default browser (handled in Rust). */
export const openExternal = (url: string) => invoke<void>('open_external', { url });
