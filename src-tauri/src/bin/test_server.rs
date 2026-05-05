//! Thin entrypoint for the HTTP test-server. The actual implementation lives
//! in `bcgagent_lib::test_server` so it can use crate-internal items (e.g.
//! `Db::lock`). Compiled only with `--features test-server`.

fn main() {
    bcgagent_lib::test_server::run_test_server();
}
