/**
 * Rules that decide when the engine removes an item for good instead of moving
 * it to the Trash. Kept in sync with the checks in `delete_path_with` in
 * src-tauri/src/cleanup_engine.rs.
 */

/** Must match the Rust constant of the same name (TRASH_MAX_BYTES). */
export const TRASH_MAX_BYTES = 5_000_000_000;

/**
 * True when the engine removes `item` permanently: it is bigger than the Trash
 * limit, it already lives in the Trash, or it is managed by its own tool
 * (a synthetic path such as "brew://old-versions").
 */
export function goesPermanent(item: CleanupItem): boolean {
  return (
    item.size_bytes > TRASH_MAX_BYTES ||
    item.type === 'Trash' ||
    item.path.includes('://')
  );
}
