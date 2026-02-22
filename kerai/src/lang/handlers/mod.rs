pub mod admin;
pub mod arithmetic;
pub mod login;
pub mod stack_ops;
pub mod workspace;

use std::collections::HashMap;

use super::machine::Handler;

/// Register all handlers, type methods, and help text.
/// Returns (global_handlers, type_methods, help).
pub fn register_all() -> (
    HashMap<String, Handler>,
    HashMap<(String, String), Handler>,
    HashMap<String, String>,
) {
    let mut handlers: HashMap<String, Handler> = HashMap::new();
    let mut type_methods: HashMap<(String, String), Handler> = HashMap::new();
    let mut help: HashMap<String, String> = HashMap::new();

    // Stack operations
    handlers.insert("dup".into(), stack_ops::dup);
    handlers.insert("drop".into(), stack_ops::drop);
    handlers.insert("swap".into(), stack_ops::swap);
    handlers.insert("over".into(), stack_ops::over);
    handlers.insert("rot".into(), stack_ops::rot);
    handlers.insert("clear".into(), stack_ops::clear);
    handlers.insert("view".into(), stack_ops::view);
    handlers.insert("depth".into(), stack_ops::depth);

    help.insert("dup".into(), "duplicate top of stack".into());
    help.insert("drop".into(), "remove top of stack".into());
    help.insert("swap".into(), "swap top two stack items".into());
    help.insert("over".into(), "copy second item to top".into());
    help.insert("rot".into(), "rotate top three items".into());
    help.insert("clear".into(), "clear the stack".into());
    help.insert("view".into(), "view top item details".into());
    help.insert("depth".into(), "push stack depth".into());

    // Arithmetic operators
    handlers.insert("+".into(), arithmetic::add);
    handlers.insert("-".into(), arithmetic::sub);
    handlers.insert("*".into(), arithmetic::mul);
    handlers.insert("/".into(), arithmetic::div);
    handlers.insert("%".into(), arithmetic::modulo);

    help.insert("+".into(), "add top two numbers".into());
    help.insert("-".into(), "subtract top from second".into());
    help.insert("*".into(), "multiply top two numbers".into());
    help.insert("/".into(), "divide second by top".into());
    help.insert("%".into(), "modulo second by top".into());

    // Library pushers
    handlers.insert("workspace".into(), workspace::workspace_lib);
    handlers.insert("login".into(), login::login_lib);
    handlers.insert("admin".into(), admin::admin_lib);

    help.insert("workspace".into(), "workspace management commands".into());
    help.insert("login".into(), "authentication commands".into());
    help.insert("admin".into(), "administration commands".into());

    // Workspace library methods
    type_methods.insert(
        ("library:workspace".into(), "list".into()),
        workspace::ws_list,
    );
    type_methods.insert(
        ("library:workspace".into(), "load".into()),
        workspace::ws_load,
    );
    type_methods.insert(
        ("library:workspace".into(), "new".into()),
        workspace::ws_new,
    );
    type_methods.insert(
        ("library:workspace".into(), "save".into()),
        workspace::ws_save,
    );

    help.insert("library:workspace/list".into(), "list all workspaces".into());
    help.insert("library:workspace/load".into(), "load workspace by number".into());
    help.insert("library:workspace/new".into(), "create a new workspace".into());
    help.insert("library:workspace/save".into(), "save current workspace with a name".into());

    // Login library methods
    type_methods.insert(
        ("library:login".into(), "bsky".into()),
        login::login_bsky,
    );

    help.insert("library:login/bsky".into(), "authenticate with Bluesky".into());

    // Admin library methods
    type_methods.insert(
        ("library:admin".into(), "oauth".into()),
        admin::admin_oauth,
    );
    type_methods.insert(
        ("library:admin.oauth".into(), "setup".into()),
        admin::admin_oauth_setup,
    );
    type_methods.insert(
        ("library:admin.oauth.setup".into(), "bsky".into()),
        admin::admin_oauth_setup_bsky,
    );
    type_methods.insert(
        ("library:admin".into(), "user".into()),
        admin::admin_user,
    );
    type_methods.insert(
        ("library:admin.user".into(), "allow".into()),
        admin::admin_user_allow,
    );

    help.insert("library:admin/oauth".into(), "OAuth configuration".into());
    help.insert("library:admin/user".into(), "user management".into());
    help.insert("library:admin.oauth/setup".into(), "OAuth setup commands".into());
    help.insert("library:admin.oauth.setup/bsky".into(), "generate ES256 keypair for Bluesky OAuth".into());
    help.insert("library:admin.user/allow".into(), "allowlist a bsky handle for login".into());

    // help command (handled as special word in execute(), not via handler map)
    help.insert("help".into(), "list all commands".into());

    (handlers, type_methods, help)
}
