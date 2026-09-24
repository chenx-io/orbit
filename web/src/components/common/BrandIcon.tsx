// Brand icon: renders the official brand SVG from simple-icons (with an optional brand color).
// Used for the official icons of each source in the import / export dialogs (cURL / Postman / OpenAPI / Swagger / JMeter / k6).
import type { SimpleIcon } from "simple-icons";

export function BrandIcon({
  icon,
  className,
  colored = false,
}: {
  icon: SimpleIcon;
  className?: string;
  /** Use the official brand color (otherwise follows currentColor so light/dark themes stay consistent) */
  colored?: boolean;
}) {
  return (
    <svg
      role="img"
      viewBox="0 0 24 24"
      className={className}
      fill={colored ? `#${icon.hex}` : "currentColor"}
      aria-hidden="true"
    >
      <path d={icon.path} />
    </svg>
  );
}
