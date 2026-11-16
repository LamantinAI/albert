import type { components } from "../../api/schema";
export type Approval = components["schemas"]["ApprovalView"];
export const terminal = (state: Approval["state"]) =>
  !["pending", "deciding", "unknown"].includes(state);
export function approvalStatus(request: Approval) {
  switch (request.state) {
    case "confirmed":
      return "Confirmed. Your decision is queued for Albert.";
    case "denied":
      return "Declined. Your decision is queued for Albert.";
    case "committed":
      return "Completed. The change was recorded.";
    case "rejected":
      return "Declined. Permission was not granted.";
    case "cancelled":
      return "This request was cancelled.";
    case "expired":
      return "This request expired. Ask Albert to prepare a new one.";
    case "superseded":
      return "This question was replaced by a newer request.";
    case "rolled_back":
      return "The change failed and was rolled back.";
    case "deciding":
      return "Recording your decision. This action will not be sent twice.";
    case "unknown":
      return "The outcome is not confirmed. Check its status before continuing; the action will not be sent again.";
    default:
      return request.kind === "confirmation"
        ? "Your decision will continue the original conversation."
        : "Review the action and its effects before deciding.";
  }
}
