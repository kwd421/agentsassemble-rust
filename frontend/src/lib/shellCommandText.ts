/**
 * Renders an argument list as one line a person can paste into a shell.
 *
 * The app runs installs from this exact argument list, so quoting only matters for the copy:
 * an npm prefix like `C:\Users\Jane Doe\...` must stay one argument when pasted. Double quotes
 * are what cmd and PowerShell both accept around a path, and an argument that already contains
 * one is escaped for PowerShell's parser rather than silently changed.
 */
export function shellCommandText(command: readonly string[]): string {
  return command.map(quoteArgument).join(" ");
}

function quoteArgument(argument: string): string {
  if (argument && !/[\s"'`$&|<>^;,()]/u.test(argument)) return argument;
  return `"${argument.replaceAll('"', '`"')}"`;
}
