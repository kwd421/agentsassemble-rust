export default function MemberUsage({ displayName }: { displayName: string }) {
  return (
    <section className="dc-member-detail-section" aria-label={`${displayName} 사용량`}>
      <h3>사용량</h3>
      <p className="dc-member-detail-note preserve-words">
        이 Provider는 확인 가능한 정확한 잔여량을 제공하지 않습니다.
      </p>
    </section>
  );
}
