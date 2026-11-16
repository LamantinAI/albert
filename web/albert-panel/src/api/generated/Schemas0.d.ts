/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Schemas0 {
/** @enum {string} */
        AccountRole: "owner" | "user";
AccountView: {
            canonical_user_id?: string | null;
            disabled: boolean;
            id: string;
            must_change_password: boolean;
            /** Format: int64 */
            revision: number;
            role: components["schemas"]["AccountRole"];
            username: string;
        };
ApiErrorBody: {
            error: components["schemas"]["ApiErrorDetail"];
        };
ApiErrorDetail: {
            code: string;
            message: string;
        };
ApplyRequest: {
            client_request_id: string;
            document_id?: string | null;
            revision?: string | null;
            /** @description runtime applies agent settings; models reloads only the configured model pool; process restarts Albert. */
            target: string;
        };
ApplyView: {
            id: string;
            message: string;
            revision: string;
            runtime_epoch: string;
            state: string;
        };
ApprovalDecision: {
            approve: boolean;
        };
/** @enum {string} */
        ApprovalKind: "confirmation" | "native";
ApprovalList: {
            items: components["schemas"]["ApprovalView"][];
            warnings?: string[];
        };
/** @enum {string} */
        ApprovalState: "pending" | "deciding" | "confirmed" | "denied" | "committed" | "rejected" | "cancelled" | "expired" | "rolled_back" | "unknown" | "superseded";
ApprovalView: {
            action: string;
            can_decide: boolean;
            connector: string;
            details?: unknown;
            effects: string[];
            /** Format: int64 */
            expires_at?: number | null;
            id: string;
            kind: components["schemas"]["ApprovalKind"];
            session_id?: string | null;
            state: components["schemas"]["ApprovalState"];
            summary: unknown;
            title: string;
        };
AttachmentView: {
            content_type: string;
            filename: string;
            id: string;
            size: number;
        };
AttemptView: {
            id: string;
            session_id: string;
            state: string;
        };
/** Format: binary */
        BinaryContent: string;
CancelRequest: {
            session_id: string;
        };
Capabilities: {
            chat: boolean;
            manage_config: boolean;
            manage_models: boolean;
            manage_skills: boolean;
            manage_users: boolean;
            owner: boolean;
        };
ClientContext: {
            account?: null | components["schemas"]["AccountView"];
            caps: components["schemas"]["Capabilities"];
            /** Format: int64 */
            credential_expires_at?: number | null;
            csrf?: string | null;
            current_credential_id?: string | null;
            runtime_epoch: string;
            setup: components["schemas"]["SetupView"];
        };
CompactCreate: {
            client_request_id: string;
            instructions?: string;
        };
ComposerSkill: {
            command?: string | null;
            description: string;
            id: string;
            name: string;
            revision: string;
        };
ComposerView: {
            can_compact: boolean;
            can_manage_credentials: boolean;
            can_reload_models: boolean;
            can_restart: boolean;
            can_select_skills: boolean;
            max_attachments: number;
            max_file_bytes: number;
            max_selected_skills: number;
            /** Format: int64 */
            recording_max_seconds: number;
            skills: components["schemas"]["ComposerSkill"][];
            speech_available: boolean;
        };
ConfigDocument: {
            content: string;
            path: string;
            redacted: boolean;
            revision: string;
        };
ConfigDraftInput: {
            changes?: components["schemas"]["ConfigFieldChange"][];
            content: string;
            revision: string;
        };
ConfigDraftView: {
            content: string;
            fields: components["schemas"]["ConfigField"][];
            revision: string;
        };
ConfigField: {
            choices?: string[] | null;
            default_value: unknown;
            description: string;
            group: string;
            kind: components["schemas"]["ConfigFieldKind"];
            label: string;
            /** Format: int64 */
            maximum?: number | null;
            /** Format: int64 */
            minimum?: number | null;
            path: string;
            required?: boolean | null;
            value?: unknown;
        };
ConfigFieldChange: {
            path: string;
            /** @description Null removes the explicit setting and restores its default. */
            value: unknown;
        };
/** @enum {string} */
        ConfigFieldKind: "boolean" | "integer" | "number" | "text" | "text_list";
ConfigList: {
            items: string[];
            revision: string;
        };
ConfigWrite: {
            content: string;
            path: string;
            revision?: string | null;
        };
ConnectorList: {
            available_types: components["schemas"]["ConnectorTypeView"][];
            items: components["schemas"]["ConnectorView"][];
        };
ConnectorTypeView: {
            kind: string;
            label: string;
        };
ConnectorView: {
            id: string;
            kind: string;
            label: string;
            settings_mode: string;
        };
CredentialCreate: {
            client_request_id: string;
            name?: string;
            session_id?: string | null;
        };
CredentialRequestList: {
            existing_names: string[];
            items: components["schemas"]["CredentialRequestView"][];
        };
CredentialRequestView: {
            /** Format: int64 */
            expires_at: number;
            id: string;
            name?: string | null;
            notification: string;
            replaced: boolean;
            replayed?: boolean;
            session_id?: string | null;
            source: string;
            state: string;
            suggested_name: string;
        };
/** @description This write-only input is deliberately not Debug/Serialize and never enters events. */
        CredentialSubmission: {
            name: string;
            value: string;
        };
CredentialUpdate: {
            current_password: string;
            new_password: string;
            new_username?: string | null;
        };
CredentialView: {
            /** Format: int64 */
            expires_at: number;
            id: string;
        };
EmptyObject: Record<string, never>;
IdentityList: {
            items: components["schemas"]["IdentityView"][];
        };
IdentityView: {
            connector: string;
            external_actor_id: string;
        };
LoginRequest: {
            password: string;
            username: string;
        };
}
