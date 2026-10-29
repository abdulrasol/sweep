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
    /** Scan-screen card id that produced the item, e.g. "xcode" or "node". */
    module: string;
    /** Framework or tool, e.g. "Flutter". */
    file_type: string;
    size_bytes: number;
    status: SafetyLevel;
    description: string;
    /** App that must be closed before this item can be removed. */
    blocked_by: string | null;
    /** Unix seconds of the newest project activity; null for global items. */
    last_active: number | null;
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

  /** A set of identical files found by the duplicate search (PLAN 4.3b), oldest first. */
  interface DuplicateGroup {
    /** Space one copy takes on disk. */
    size_each: number;
    /** Space freed by removing every copy but one. */
    wasted: number;
    items: CleanupItem[];
  }

  /** Totals for one tool in the cleanup history (PLAN 4.5). */
  interface HistoryModule {
    module: string;
    freed: number;
    trashed: number;
    count: number;
  }

  /** Totals for one UTC day (YYYY-MM-DD) in the cleanup history. */
  interface HistoryDay {
    day: string;
    freed: number;
    trashed: number;
    count: number;
  }

  interface HistorySummary {
    /** Bytes removed for good. */
    total_freed: number;
    /** Bytes moved to the Trash; not free until the Trash is emptied. */
    total_trashed: number;
    items_removed: number;
    /** Unix seconds of the first recorded removal. */
    first_ts: number | null;
    last_30_days_freed: number;
    by_module: HistoryModule[];
    /** Last 30 days only, oldest first; days without removals are missing. */
    by_day: HistoryDay[];
  }

  type View = 'scan' | 'review' | 'cleanup' | 'large' | 'history' | 'settings' | 'about';
}
