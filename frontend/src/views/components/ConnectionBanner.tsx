export default function ConnectionBanner({ message }: { message: string }) {
  return <div role="status" className="dc-connection-banner"><span aria-hidden>●</span> {message}</div>;
}
