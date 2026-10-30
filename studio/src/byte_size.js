// byte_size.js
// Byte counts as the sizes the UI shows. One set of units for every tab
// (1024-based, as Windows Explorer reports them), so a size isn't divided by a
// different magic number in each file (#1107 found the same arithmetic in a
// dozen places).

import { STRINGS } from './strings.js';

export const BYTES_PER_KB = 1024;
export const BYTES_PER_MB = 1024 ** 2;
export const BYTES_PER_GB = 1024 ** 3;
export const BYTES_PER_TB = 1024 ** 4;

/** A size as "3.00 GB" from a gigabyte up, "20.9 MB" below it: the Demo
 *  Auditor's wasted space and the batch results' take sizes. */
export function gbOrMbText(bytes) {
  const gb = bytes / BYTES_PER_GB;
  return gb >= 1 ? `${gb.toFixed(2)} GB` : `${(bytes / BYTES_PER_MB).toFixed(1)} MB`;
}

/** A size as the number of gigabytes to two places, "12.34", for strings that
 *  add the "GB" themselves: the Capture tab's space warnings (#1181). */
export function gigabytes(bytes) {
  return (bytes / BYTES_PER_GB).toFixed(2);
}

/** A file's size in the Render tab's words: GB to `gbDecimals` places from a
 *  gigabyte up, whole MB below it. Anything that isn't a number counts as 0. */
export function renderSizeText(bytes, gbDecimals) {
  const n = Number(bytes) || 0;
  const gb = n / BYTES_PER_GB;
  return gb >= 1
    ? STRINGS.RENDER.fileSizeGb(gb.toFixed(gbDecimals))
    : STRINGS.RENDER.fileSizeMb((n / BYTES_PER_MB).toFixed(0));
}
