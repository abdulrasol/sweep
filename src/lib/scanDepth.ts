/** Project-walk depth choices shown in Settings. Mirrors the backend clamp (2..=12). */
export const SEARCH_DEPTHS = [4, 6, 8, 10, 12] as const;

export const DEFAULT_SEARCH_DEPTH = 8;

export const isSearchDepth = (value: number) =>
  (SEARCH_DEPTHS as readonly number[]).includes(value);
