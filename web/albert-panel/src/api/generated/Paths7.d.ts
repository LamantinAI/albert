/** Generated from the Rust Utoipa schema via xtask. Do not edit. */
import type { components, operations } from '../schema'

export interface Paths7 {
"/users/{id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch: {
            parameters: {
                query?: never;
                header: {
                    "x-albert-csrf": string;
                };
                path: {
                    id: string;
                };
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["UserUpdate"];
                };
            };
            responses: {
                /** @description Success */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["AccountView"];
                    };
                };
                /** @description Error */
                default: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["ApiErrorBody"];
                    };
                };
            };
        };
        trace?: never;
    };
}
