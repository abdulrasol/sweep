/** Arabic plural forms for one message, as base_zero ... base_other keys. */
export function arPlural(
  base: string,
  f: { zero: string; one: string; two: string; few: string; many: string; other: string },
): Record<string, string> {
  return {
    [`${base}_zero`]: f.zero,
    [`${base}_one`]: f.one,
    [`${base}_two`]: f.two,
    [`${base}_few`]: f.few,
    [`${base}_many`]: f.many,
    [`${base}_other`]: f.other,
  };
}
