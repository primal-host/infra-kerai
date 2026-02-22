use crate::lang::machine::Machine;
use crate::lang::ptr::Ptr;

/// `admin` — push the admin library marker.
pub fn admin_lib(m: &mut Machine) -> Result<(), String> {
    m.push(Ptr::library("admin"));
    Ok(())
}

/// `admin oauth` — push the admin.oauth library marker.
pub fn admin_oauth(m: &mut Machine) -> Result<(), String> {
    m.push(Ptr::library("admin.oauth"));
    Ok(())
}

/// `admin oauth setup` — push the admin.oauth.setup library marker.
pub fn admin_oauth_setup(m: &mut Machine) -> Result<(), String> {
    m.push(Ptr::library("admin.oauth.setup"));
    Ok(())
}

/// `admin oauth setup bsky` — request to generate ES256 keypair and store in config.
/// Pushes a request marker that eval.rs resolves asynchronously.
pub fn admin_oauth_setup_bsky(m: &mut Machine) -> Result<(), String> {
    m.push(Ptr {
        kind: "admin_oauth_setup_request".into(),
        ref_id: "bsky".into(),
        meta: serde_json::Value::Null,
        id: 0,
    });
    Ok(())
}

/// `admin user` — push the admin.user library marker.
pub fn admin_user(m: &mut Machine) -> Result<(), String> {
    m.push(Ptr::library("admin.user"));
    Ok(())
}

/// `admin user allow` — pop text from stack, push admin_user_allow_request.
/// Usage: `"handle.bsky.social" admin user allow`
pub fn admin_user_allow(m: &mut Machine) -> Result<(), String> {
    let handle_ptr = m.pop().ok_or("admin user allow: need a handle on the stack")?;
    if handle_ptr.kind != "text" {
        return Err(format!(
            "admin user allow: expected text, got {}",
            handle_ptr.kind
        ));
    }
    let handle = handle_ptr.ref_id;
    m.push(Ptr {
        kind: "admin_user_allow_request".into(),
        ref_id: handle,
        meta: serde_json::Value::Null,
        id: 0,
    });
    Ok(())
}
