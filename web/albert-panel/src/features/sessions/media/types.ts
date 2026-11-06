import type { components } from "../../../api/schema";
import { apiPath } from "../../../api/paths";

export type Attachment = components["schemas"]["AttachmentView"];
export function assetUrl(session: string, asset: Attachment) {
  return apiPath(
    `/sessions/${encodeURIComponent(session)}/attachments/${encodeURIComponent(asset.id)}`,
  );
}
export function fileSize(size: number) {
  return size < 1024 * 1024
    ? `${Math.max(1, Math.round(size / 1024))} KB`
    : `${(size / 1024 / 1024).toFixed(1)} MB`;
}
