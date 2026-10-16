import googleGLogo from "../../assets/google-g.svg";

// The official colour Google "G" shared by every Google sign-in button.
// Branding contract: docs/specs/identity-accounts-friends-slice.md
// ("Google sign-in button branding"). The logo is never stretched,
// recoloured or shown without its button boundary and action text.
export default function GoogleGLogo({ size = 18 }: { size?: number }) {
  return (
    <img
      src={googleGLogo}
      alt=""
      aria-hidden="true"
      draggable={false}
      width={size}
      height={size}
      style={{ flex: "0 0 auto", width: size, height: size }}
    />
  );
}
