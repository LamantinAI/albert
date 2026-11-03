/** Generated from contracts/web-panel.openapi.json. Do not edit. */
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
ApprovalList: {
            items: components["schemas"]["ApprovalView"][];
        };
ApprovalView: {
            connector: string;
            /** Format: int64 */
            expires_at?: number | null;
            id: string;
            state: string;
            summary: unknown;
        };
AttemptView: {
            id: string;
            session_id: string;
            state: string;
        };
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
            csrf?: string | null;
            current_credential_id?: string | null;
            runtime_epoch: string;
            setup: components["schemas"]["SetupView"];
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
            value?: unknown;
        };
ConfigFieldChange: {
            path: string;
            /** @description Null removes the explicit setting and restores its default. */
            value: unknown;
        };
/** @enum {string} */
        ConfigFieldKind: "boolean" | "integer" | "text";
ConfigList: {
            items: string[];
            revision: string;
        };
ConfigWrite: {
            content: string;
            path: string;
            revision?: string | null;
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
MessageCreate: {
            client_message_id: string;
            digest?: string | null;
            text: string;
        };
MessageList: {
            has_more: boolean;
            items: components["schemas"]["MessageView"][];
            next_cursor?: string | null;
            through_id?: string | null;
        };
MessageReceipt: {
            attempt_id?: string | null;
            client_message_id: string;
            duplicate: boolean;
            message_ids: string[];
        };
MessageView: {
            content: string;
            created_at: string;
            id: string;
            role: string;
        };
ModelConfig: {
            api_key_env?: string | null;
            base_url?: string | null;
            context_window?: number | null;
            id: string;
            model: string;
            provider: string;
            reasoning?: unknown;
            /** Format: int64 */
            request_timeout_ms?: number | null;
            tools: boolean;
            vision: boolean;
        };
ModelOrder: {
            ids: string[];
            revision: string;
        };
ModelPoolView: {
            models: components["schemas"]["ModelView"][];
            preferred?: string | null;
            revision: string;
            roles: components["schemas"]["ModelRole"][];
        };
ModelPreference: {
            model_id: string;
        };
ModelRole: {
            description: string;
            id: string;
            models: string[];
            reasoning_effort?: string | null;
        };
ModelView: {
            config: components["schemas"]["ModelConfig"];
            credential_configured: boolean;
            priority: number;
        };
ModelWrite: {
            api_key?: string | null;
            config: components["schemas"]["ModelConfig"];
            revision?: string | null;
        };
/** @enum {string} */
        OpenAiLoginPhase: "awaiting_callback" | "exchanging" | "connected" | "failed" | "cancelled" | "expired";
OpenAiLoginStart: {
            replace_existing?: boolean;
        };
OpenAiLoginSubmission: {
            callback: string;
        };
OpenAiLoginView: {
            authorize_url?: string | null;
            credentials_saved: boolean;
            expires_at: string;
            id: string;
            message?: string | null;
            phase: components["schemas"]["OpenAiLoginPhase"];
            redirect_uri: string;
        };
PageQuery: {
            after?: string | null;
            limit?: number | null;
            order?: string | null;
            through_id?: string | null;
        };
ProfileView: {
            canonical_user_id?: string | null;
            display_name?: string | null;
        };
ProgressDelta: {
            at: string;
            attempt_id: string;
            items: components["schemas"]["ProgressItem"][];
            replace_kind?: string | null;
        };
ProgressItem: {
            /** Format: int64 */
            elapsed_ms?: number | null;
            id: string;
            kind: string;
            label: string;
            started_at: string;
            state: string;
            updated_at: string;
        };
ProgressSnapshot: {
            attempt_id: string;
            items: components["schemas"]["ProgressItem"][];
            started_at: string;
            truncated: boolean;
            updated_at: string;
        };
RuntimeStatus: {
            error?: string | null;
            model_ready: boolean;
            phase: string;
            runtime_epoch: string;
        };
SecretList: {
            items: string[];
        };
SecretWrite: {
            value: string;
        };
SessionCreate: {
            temporary?: boolean;
            title?: string | null;
        };
SessionEvent: {
            attempt_id?: string | null;
            data: unknown;
            id: string;
            kind: string;
            runtime_epoch: string;
            session_id: string;
        };
SessionList: {
            has_more: boolean;
            items: components["schemas"]["SessionView"][];
            next_cursor?: string | null;
        };
SessionStart: {
            client_message_id: string;
            temporary?: boolean;
            text: string;
        };
SessionStartView: {
            id: string;
            published: boolean;
            session_id: string;
            state: string;
            temporary: boolean;
        };
SessionState: {
            active_attempt?: null | components["schemas"]["AttemptView"];
            progress?: null | components["schemas"]["ProgressSnapshot"];
            runtime_epoch: string;
            session: components["schemas"]["SessionView"];
            status: string[];
            text?: null | components["schemas"]["TextSnapshot"];
        };
SessionUpdate: {
            archived?: boolean | null;
            kind?: string | null;
            promotion_key?: string | null;
            /** Format: int64 */
            revision: number;
            state?: string | null;
            title?: string | null;
        };
SessionView: {
            created_at: string;
            host: string;
            id: string;
            kind: string;
            /** Format: int64 */
            revision: number;
            state: string;
            title: string;
        };
/** @enum {string} */
        SetupPhase: "credentials" | "profile" | "model" | "ready" | "runtime_error";
SetupView: {
            identity_required: boolean;
            model_ready: boolean;
            phase: components["schemas"]["SetupPhase"];
        };
SkillImport: {
            data_base64: string;
            filename: string;
            replace?: boolean;
        };
SkillList: {
            items: components["schemas"]["SkillView"][];
        };
SkillUpdate: {
            enabled: boolean;
            revision: string;
        };
SkillView: {
            command?: string | null;
            content?: string | null;
            description: string;
            enabled: boolean;
            id: string;
            name: string;
            owner_only: boolean;
            revision: string;
        };
SkillWrite: {
            content: string;
            revision?: string | null;
        };
SubscriptionApplyRequest: {
            generation: string;
        };
SubscriptionCatalogModel: {
            context_window?: number | null;
            default_reasoning_effort?: string | null;
            description?: string | null;
            input_modalities: string[];
            name: string;
            reasoning_efforts: string[];
            selectable: boolean;
            slug: string;
            unavailable_reason?: string | null;
        };
SubscriptionCatalogView: {
            generation: string;
            models: components["schemas"]["SubscriptionCatalogModel"][];
            revision: string;
        };
SubscriptionConnectionView: {
            connected: boolean;
            generation?: string | null;
            message?: string | null;
            phase: components["schemas"]["SubscriptionPhase"];
            preferred_model_id?: string | null;
            selected_model_id?: string | null;
            selected_slug?: string | null;
        };
/** @enum {string} */
        SubscriptionPhase: "disconnected" | "connected" | "applying" | "ready" | "apply_failed";
SubscriptionSelection: {
            catalog_revision: string;
            generation: string;
            prefer?: boolean;
            slug: string;
        };
TextDelta: {
            attempt_id: string;
            generation: string;
            message_ids: string[];
            offset: number;
            operation: string;
            text: string;
        };
TextSnapshot: {
            attempt_id: string;
            generation: string;
            message_ids: string[];
            offset: number;
            state: string;
            text: string;
            truncated: boolean;
        };
UserCreate: {
            password: string;
            role: components["schemas"]["AccountRole"];
            username: string;
        };
UserList: {
            items: components["schemas"]["AccountView"][];
        };
UserUpdate: {
            disabled?: boolean | null;
            /** Format: int64 */
            revision: number;
            role?: null | components["schemas"]["AccountRole"];
        };
}
