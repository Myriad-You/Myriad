//! Package IO and installation use cases shared by HTTP and Agent callers.
mod access;
mod installation;
pub(crate) mod package_files;
pub(crate) mod prepared_package;
pub(crate) mod runtime;
pub(crate) mod widgets;
pub(crate) use access::{ensure_permissions_allowed, ensure_tapp_install_allowed};
pub(crate) use installation::{
    acquire_install_permit, install_generated, install_prepared_package, resolve_update_target,
    update_prepared_package,
};
