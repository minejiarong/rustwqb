pub mod app_command;
pub mod backtest;
pub mod catch;

pub use app_command::AppCommand;

use crate::session::WQBSession;
use crate::AppEvent;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::mpsc;

// Deprecated: logic moved to AppCommand handling in main.rs or new handler
// We will keep this for now but it might be replaced by the loop in main.rs handling AppCommand
pub async fn handle_command_legacy(
    cmd: &str,
    session: Option<Arc<WQBSession>>,
    db: Arc<DatabaseConnection>,
    evt_tx: mpsc::UnboundedSender<AppEvent>,
) {
    // ... original implementation if needed ...
}
