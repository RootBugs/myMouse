import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const projectRoot = resolve(__dirname, '..');

describe('Electron file:// asset loading', () => {
  it('uses a relative Vite base so built assets resolve from file://', () => {
    const config = readFileSync(resolve(projectRoot, 'vite.config.ts'), 'utf8');
    expect(config).toMatch(/base:\s*['"]\.\/['"]/);
  });

  it('does not build index.html with root-absolute asset URLs', () => {
    const html = readFileSync(resolve(projectRoot, 'dist/index.html'), 'utf8');
    expect(html).not.toMatch(/(?:src|href)="\//);
    expect(html).toMatch(/(?:src|href)="\.\/assets\//);
  });
});
