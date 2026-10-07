export {};

declare global {
  type SafetyLevel = 'SAFE' | 'REVIEW' | 'DANGER';

  interface CleanupItem {
    id: string;
    path: string;
    /** Artifact kind, e.g. "Build Cache". */
    type: string;
    /** Filter bucket: Projects, Dev Tools, AI Tools, Editors, System & Apps. */
    group: string;
    /** Framework or tool, e.g. "Flutter". */
    file_type: string;
    size_bytes: number;
    status: SafetyLevel;
    description: string;
    /** App that must be closed before this item can be removed. */
    blocked_by: string | null;
  }

  interface SystemInfo {
    os_name: string;
    os_version: string;
    cpu_usage: number;
    ram_total: number;
    ram_used: number;
    disk_total: number;
    disk_free: number;
  }

  type View = 'scan' | 'review' | 'cleanup' | 'settings' | 'about';
}
