import { review } from './en/review';
import { modules } from './en/modules';
import { shell } from './en/shell';
import { misc } from './en/misc';
import { files } from './en/files';
import { diskMap } from './en/map';

/** English is the source of truth: every key here must exist in ar.ts. */
export const en = { ...review, ...modules, ...shell, ...misc, ...files, ...diskMap } as const;

export type MessageKey = keyof typeof en;
