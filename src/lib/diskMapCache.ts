/**
 * Sizes already measured for the disk map, by folder path. Walking back to a
 * folder shows the saved result at once; the cache is dropped when the user
 * asks for a refresh or when a cleanup may have changed the disk.
 */
const cache = new Map<string, DiskMap>();

export const getCachedMap = (path: string): DiskMap | undefined => cache.get(path);

/** Saves a map under the path that was asked for and the one it resolved to. */
export function cacheMap(requested: string, map: DiskMap): void {
  cache.set(requested, map);
  cache.set(map.path, map);
}

export const clearDiskMapCache = (): void => cache.clear();
