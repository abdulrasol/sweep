/**
 * Update check against the website's API (web/functions/api/latest.js).
 * Sweep never downloads or installs anything itself: it only tells the user and
 * opens the download link in the browser.
 */

export const UPDATE_API = 'https://sweep.subultech.top/api/latest';
export const RELEASES_URL = 'https://github.com/abdulrasol/sweep/releases';

export type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'current'; checkedAt: number }
  | { kind: 'available'; version: string; downloadUrl: string; releaseUrl: string }
  | { kind: 'error' };

interface LatestRelease {
  version?: string;
  release_url?: string;
  downloads?: { macos?: { url?: string } };
  update_available?: boolean;
}

/** Parse "1.2.3" or "v1.2.3"; anything after the patch number is ignored. */
export const parseVersion = (v: string): [number, number, number] | null => {
  const m = v.trim().replace(/^v/i, '').match(/^(\d+)\.(\d+)\.(\d+)/);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
};

/** True only when `latest` is strictly newer than `current`. */
export const isNewer = (latest: string, current: string): boolean => {
  const a = parseVersion(latest);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return false;
};

export async function checkForUpdate(current: string, signal?: AbortSignal): Promise<UpdateState> {
  const res = await fetch(`${UPDATE_API}?current=${encodeURIComponent(current)}`, {
    headers: { Accept: 'application/json' },
    signal,
  });
  if (res.status === 404) return { kind: 'current', checkedAt: Date.now() }; // nothing published yet
  if (!res.ok) throw new Error(`Update server returned ${res.status}`);

  const data = (await res.json()) as LatestRelease;
  const latest = data.version ?? '';
  // Trust our own comparison too, so a server bug can't offer an older version.
  if (latest && isNewer(latest, current)) {
    const releaseUrl = data.release_url || RELEASES_URL;
    return {
      kind: 'available',
      version: latest,
      downloadUrl: data.downloads?.macos?.url || releaseUrl,
      releaseUrl,
    };
  }
  return { kind: 'current', checkedAt: Date.now() };
}
