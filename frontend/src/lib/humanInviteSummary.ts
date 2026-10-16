function duration(seconds: number): string {
  // Up to two days reads in hours ("24시간"), as the invite options always have.
  if (seconds % 3600 === 0 && seconds <= 172800) return `${seconds / 3600}시간`;
  if (seconds % 86400 === 0) return `${seconds / 86400}일`;
  if (seconds % 3600 === 0) return `${seconds / 3600}시간`;
  if (seconds % 60 === 0) return `${seconds / 60}분`;
  return `${seconds}초`;
}

/** One sentence on who can use a human invite link and when it stops working. */
export function humanInviteSummary(maxUses: number, ttlSeconds: number): string {
  const uses = maxUses === 0 ? "인원 제한 없이" : maxUses === 1 ? "1명만" : `${maxUses}명까지`;
  return `${uses} 쓸 수 있고 ${duration(ttlSeconds)} 후 만료돼요.`;
}
