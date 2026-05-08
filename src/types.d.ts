export {};

declare global {
  interface CleanupItem {
    id: string;
    path: string;
    type: string;
    file_type?: string;
    category?: string;
    size: string;
    size_bytes: number;
    status: 'SAFE' | 'WARNING' | 'DANGER' | 'REVIEW' | 'UNSAFE';
    description: string;
  }

  interface Window {
    __TAURI__: any;
  }
}
