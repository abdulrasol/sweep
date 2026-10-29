import { review } from './en/review';
import { modules } from './en/modules';
import { shell } from './en/shell';
import { misc } from './en/misc';
import { files } from './en/files';

/** English is the source of truth: every key here must exist in ar.ts. */
export const en = { ...review, ...modules, ...shell, ...misc, ...files } as const;

export type MessageKey = keyof typeof en;
