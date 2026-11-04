import { LampLoader } from "./LampLoader";

export function ResourceLoading({ label }: { label: string }) {
  return (
    <div className="resource-loading">
      <LampLoader label={label} />
      <span aria-hidden="true">{label}</span>
    </div>
  );
}
