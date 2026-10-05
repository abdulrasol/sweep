export {};

declare global {
  type SafetyLevel = 'SAFE' | 'REVIEW' | 'DANGER';

  interface CleanupItem {
    id: string;
    path: string;
    /** Artifact kind, e.g. "Build Cache". */
    type: string;
    /** Framework or tool, e.g. "Flutter". */
    file_type: string;
    size_bytes: number;
    status: SafetyLevel;
    description: string;
    /** App that must be closed before this item can be removed. */
    blocked_by: string | null;
  }
}
