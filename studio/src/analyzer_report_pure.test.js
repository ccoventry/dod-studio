// The report viewer is shared with the web analyzer (#238), which has no
// Tauri: these modules must not import it, directly or through ipc_bridge.js.
import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const dir = path.dirname(fileURLToPath(import.meta.url));

describe('the shared report half stays free of Tauri', () => {
  it.each(['analyzer_report.js', 'analyzer_report_logic.js'])('%s', (file) => {
    const src = fs.readFileSync(path.join(dir, file), 'utf8');
    const imports = [...src.matchAll(/^\s*(?:import|export)\b[^;]*?from\s+'([^']+)'/gm)].map((m) => m[1]);
    expect(imports.filter((i) => i.startsWith('@tauri-apps/') || i.includes('ipc_bridge'))).toEqual([]);
  });
});
