/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Schemas3 {
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
ValidationView: {
            issues: string[];
            requires_restart: boolean;
            valid: boolean;
        };
}
