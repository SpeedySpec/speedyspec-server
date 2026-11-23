//! Legacy XML-RPC methods (`xmlrpc_methods`), as operations.

use speedyspec_xmlrpc_contracts::MethodEntry;

/// The served method table. Until the `wp_xmlrpc_server` shim builds it, the exposed operations as they are.
pub(crate) fn methods(table: Vec<MethodEntry>) -> Vec<MethodEntry> {
    table
}
