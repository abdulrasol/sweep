import type { MessageKey } from './en';
import { review } from './ar/review';
import { modules } from './ar/modules';
import { shell } from './ar/shell';
import { misc } from './ar/misc';
import { files } from './ar/files';

// The type makes a missing translation a compile error. Extra keys are the Arabic plural forms.
export const ar: Record<MessageKey, string> & Record<string, string> = {
  ...review, ...modules, ...shell, ...misc, ...files,
} as Record<MessageKey, string> & Record<string, string>;
