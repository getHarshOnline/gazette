/** Present the native check report without interpreting publication rules in the editor. */
export function workspaceCheckText(raw: string): string {
  const value = JSON.parse(raw)?.result?.structuredContent;
  if (!value || typeof value !== 'object') throw new Error('Gazette returned no check report.');
  if (typeof value.code === 'string' && typeof value.message === 'string') {
    return `Gazette check could not finish\n\n${value.code}: ${value.message}\n`;
  }
  if (typeof value.workspace_location?.root !== 'string'
      || !Array.isArray(value.checked_sites) || !value.checked_sites.every((site: unknown) => typeof site === 'string')
      || !Array.isArray(value.diagnostics)) throw new Error('Gazette returned an invalid check report.');
  const problems = value.diagnostics.map((row: Record<string, unknown>) => {
    if (!row || typeof row.code !== 'string' || typeof row.message !== 'string'
        || (row.file !== null && typeof row.file !== 'string')) throw new Error('Gazette returned an invalid problem.');
    let location = row.file ?? '';
    if (location && Number.isSafeInteger(row.line) && (row.line as number) > 0) {
      location += `:${row.line}`;
      if (Number.isSafeInteger(row.column) && (row.column as number) > 0) location += `:${row.column}`;
    }
    return `${location ? `${location}: ` : ''}${row.code}: ${row.message.replace(/[\r\n]/g, ' ')}`;
  });
  return [
    'Gazette: Check Saved Site',
    `Workspace: ${value.workspace_location.root}`,
    `Sites checked: ${value.checked_sites.length}`,
    problems.length ? `Problems found: ${problems.length}` : 'No problems found.',
    '', ...problems, '',
  ].join('\n');
}
