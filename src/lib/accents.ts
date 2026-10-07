/** Accent colors offered in the header and in Settings. */
export const ACCENTS = [
  { id: 'emerald', label: 'Emerald', hex: '#10b981', rgb: '16, 185, 129' },
  { id: 'blue', label: 'Blue', hex: '#3b82f6', rgb: '59, 130, 246' },
  { id: 'violet', label: 'Violet', hex: '#8b5cf6', rgb: '139, 92, 246' },
  { id: 'rose', label: 'Rose', hex: '#f43f5e', rgb: '244, 63, 94' },
  { id: 'amber', label: 'Amber', hex: '#f59e0b', rgb: '245, 158, 11' },
] as const;

export const accentById = (id: string) => ACCENTS.find(a => a.id === id) ?? ACCENTS[0];
