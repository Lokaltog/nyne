// Static LSP client configuration: environment variables and capability declarations.

use lsp_types::{
    CallHierarchyClientCapabilities, ClientCapabilities, CodeActionCapabilityResolveSupport,
    CodeActionClientCapabilities, CodeActionKind, CodeActionKindLiteralSupport, CodeActionLiteralSupport,
    DiagnosticClientCapabilities, GotoCapability, InlayHintClientCapabilities, PublishDiagnosticsClientCapabilities,
    ReferenceClientCapabilities, RenameClientCapabilities, TextDocumentClientCapabilities,
    TextDocumentSyncClientCapabilities, TypeHierarchyClientCapabilities, WorkspaceSymbolClientCapabilities,
};

/// Environment variables propagated from the parent process into LSP server
/// subprocesses. Everything else is cleared to prevent shell hooks (direnv,
/// conda, nvm, ...) from activating and potentially accessing the FUSE mount.
pub(super) const PROPAGATED_ENV_VARS: &[&str] = &[
    // Core POSIX
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "TERM",
    // Nix / NixOS — required for Nix-managed toolchains to resolve store paths
    "NIX_PATH",
    "NIX_PROFILES",
    "NIX_SSL_CERT_FILE",
    "LOCALE_ARCHIVE",
    // TLS — language servers fetching crates/packages need CA bundles
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    // Rust toolchain
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
];

/// Static goto capability — reused for definition, declaration,
/// `type_definition`, and implementation (all identical).
const GOTO_CAPABILITY: GotoCapability = GotoCapability {
    dynamic_registration: Some(false),
    link_support: Some(false),
};

/// Build the client capabilities we advertise to servers.
pub(super) fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                dynamic_registration: Some(false),
                will_save: Some(false),
                will_save_wait_until: Some(false),
                did_save: Some(false),
            }),
            references: Some(ReferenceClientCapabilities {
                dynamic_registration: Some(false),
            }),
            rename: Some(RenameClientCapabilities {
                dynamic_registration: Some(false),
                prepare_support: Some(true),
                ..Default::default()
            }),
            call_hierarchy: Some(CallHierarchyClientCapabilities {
                dynamic_registration: Some(false),
            }),
            definition: Some(GOTO_CAPABILITY),
            declaration: Some(GOTO_CAPABILITY),
            type_definition: Some(GOTO_CAPABILITY),
            implementation: Some(GOTO_CAPABILITY),
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities::default()),
            diagnostic: Some(DiagnosticClientCapabilities {
                dynamic_registration: Some(false),
                related_document_support: Some(false),
            }),
            inlay_hint: Some(InlayHintClientCapabilities {
                dynamic_registration: Some(false),
                resolve_support: None,
            }),
            type_hierarchy: Some(TypeHierarchyClientCapabilities {
                dynamic_registration: Some(false),
            }),
            code_action: Some(CodeActionClientCapabilities {
                dynamic_registration: Some(false),
                code_action_literal_support: Some(CodeActionLiteralSupport {
                    code_action_kind: CodeActionKindLiteralSupport {
                        value_set: vec![
                            CodeActionKind::QUICKFIX.as_str().to_owned(),
                            CodeActionKind::REFACTOR.as_str().to_owned(),
                            CodeActionKind::SOURCE.as_str().to_owned(),
                        ],
                    },
                }),
                is_preferred_support: Some(true),
                disabled_support: Some(true),
                data_support: Some(true),
                resolve_support: Some(CodeActionCapabilityResolveSupport {
                    properties: vec!["edit".to_owned()],
                }),
                honors_change_annotations: None,
            }),
            ..Default::default()
        }),
        workspace: Some(lsp_types::WorkspaceClientCapabilities {
            file_operations: Some(lsp_types::WorkspaceFileOperationsClientCapabilities {
                will_rename: Some(true),
                did_rename: Some(true),
                ..Default::default()
            }),
            symbol: Some(WorkspaceSymbolClientCapabilities {
                dynamic_registration: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}
