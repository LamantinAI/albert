/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Schemas1 {
McpConnection: {
            token_env?: string | null;
            /** @enum {string} */
            transport: "http";
            url: string;
        } | {
            args?: string[];
            command: string;
            env?: {
                [key: string]: string;
            };
            inherited_env?: string[] | null;
            /** @enum {string} */
            transport: "stdio";
        };
McpServerConfig: {
            allowed_tools: string[];
            connection: components["schemas"]["McpConnection"];
            description: string;
            enabled: boolean;
            id: string;
            name: string;
        };
McpServerList: {
            items: components["schemas"]["McpServerView"][];
            revision: string;
        };
McpServerView: {
            config: components["schemas"]["McpServerConfig"];
            error?: string | null;
            phase: string;
            tool_count: number;
        };
McpServerWrite: {
            config: components["schemas"]["McpServerConfig"];
            revision: string;
        };
McpToolList: {
            items: components["schemas"]["McpToolView"][];
            missing_allowed: string[];
            next_offset?: number | null;
            offset: number;
            revision: string;
            total: number;
        };
McpToolView: {
            allowed: boolean;
            description: string;
            name: string;
            schema?: unknown;
        };
/** @enum {string} */
        MemoryBackend: "embedded" | "mcp";
MemoryDocument: {
            content: string;
            revision: string;
        };
MemoryInitialization: {
            revision: string;
        };
/** @enum {string} */
        MemoryPhase: "selection_pending" | "saved" | "initializing" | "active" | "restart_required" | "initialization_failed";
MemorySettings: {
            args?: string[] | null;
            backend: components["schemas"]["MemoryBackend"];
            command?: string | null;
            /** @description Child environment variable to saved credential name, never its value. */
            env?: {
                [key: string]: string;
            } | null;
            /** Format: int64 */
            timeout_secs?: number | null;
            token_env?: string | null;
            transport?: null | components["schemas"]["MemoryTransport"];
            url?: string | null;
        };
/** @enum {string} */
        MemoryTransport: "http" | "stdio";
MemoryView: {
            config: components["schemas"]["MemorySettings"];
            credential_names: string[];
            document?: null | components["schemas"]["ConfigDocument"];
            error?: string | null;
            model_configured: boolean;
            phase: components["schemas"]["MemoryPhase"];
            revision: string;
            selection_required: boolean;
            token_configured: boolean;
        };
MemoryWrite: {
            config: components["schemas"]["MemorySettings"];
            document?: null | components["schemas"]["MemoryDocument"];
            revision: string;
            /** @description Write-only bearer token. Omit to retain the saved value. */
            token_value?: string | null;
        };
MessageCreate: {
            attachments?: string[];
            client_message_id: string;
            digest?: string | null;
            skills?: components["schemas"]["SkillSelection"][];
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
            attachments?: components["schemas"]["AttachmentView"][];
            content: string;
            created_at: string;
            id: string;
            role: string;
            skills?: components["schemas"]["SelectedSkillView"][];
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
ModelRoleWrite: {
            description: string;
            id: string;
            model_id: string;
            revision: string;
        };
ModelUsageSummary: {
            /** Format: int64 */
            lost_records: number;
            models: components["schemas"]["ModelUsageView"][];
            pending_records: number;
            period_started_at: string;
            tracking_started_at: string;
        };
ModelUsageView: {
            /** Format: int64 */
            cached_input_tokens?: number | null;
            /** Format: int64 */
            input_tokens?: number | null;
            model: string;
            model_id: string;
            /** Format: int64 */
            output_tokens?: number | null;
            provider: string;
            /** Format: int64 */
            reported_calls: number;
            /** Format: int64 */
            total_tokens?: number | null;
            /** Format: int64 */
            unreported_calls: number;
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
OperatorCard: {
            active_children: number;
            approval_pending?: boolean | null;
            /** @description available, unavailable, or restricted; absence is never a zero count. */
            attention_state: string;
            credential_pending?: boolean | null;
            outcome?: string | null;
            running: boolean;
            session_id: string;
            steps: components["schemas"]["ProgressItem"][];
        };
OperatorQuery: {
            /** @description Comma-separated IDs from the caller's private conversation catalog (up to 100). */
            session_ids: string;
        };
OperatorSnapshot: {
            catalog_changed: boolean;
            items: components["schemas"]["OperatorCard"][];
            runtime_epoch: string;
        };
PageQuery: {
            after?: string | null;
            limit?: number | null;
            order?: string | null;
            through_id?: string | null;
        };
}
