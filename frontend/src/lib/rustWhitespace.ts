function isRustWhitespace(codePoint: number): boolean {
  return (
    (codePoint >= 0x0009 && codePoint <= 0x000d) ||
    codePoint === 0x0020 ||
    codePoint === 0x0085 ||
    codePoint === 0x00a0 ||
    codePoint === 0x1680 ||
    (codePoint >= 0x2000 && codePoint <= 0x200a) ||
    codePoint === 0x2028 ||
    codePoint === 0x2029 ||
    codePoint === 0x202f ||
    codePoint === 0x205f ||
    codePoint === 0x3000
  );
}

export function trimRustWhitespace(value: string): string {
  const scalars = [...value];
  let start = 0;
  let end = scalars.length;
  while (start < end && isRustWhitespace(scalars[start].codePointAt(0)!)) start += 1;
  while (end > start && isRustWhitespace(scalars[end - 1].codePointAt(0)!)) end -= 1;
  return scalars.slice(start, end).join("");
}
