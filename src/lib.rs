pgrx::pg_module_magic!();

mod bootstrap;
mod functions;
mod identity;
mod schema;
mod workers;

#[pgrx::pg_guard]
pub extern "C-unwind" fn _PG_init() {
    workers::register_workers();
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {}

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}
