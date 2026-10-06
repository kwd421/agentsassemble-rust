import { connectorInviteText } from "../../api/connectorInvite";

export default function ConnectorJoinNotice({ joinUrl }: { joinUrl: string }) {
  return (
    <section className="dc-invite-card" aria-labelledby="connector-join-heading" style={{ maxWidth: 560, margin: "24px auto" }}>
      <div className="dc-invite-card-copy">
        <h3 id="connector-join-heading">AI 앱 참가 안내</h3>
        <pre style={{ whiteSpace: "pre-wrap", margin: 0 }}>{connectorInviteText(joinUrl)}</pre>
      </div>
    </section>
  );
}
