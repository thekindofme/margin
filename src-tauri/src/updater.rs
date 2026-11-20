//! Self-update: check GitHub Releases for a newer signed build and install it after asking.
use crate::recorder;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

/// Check for an update in the background. `manual` checks also report "up to date" and errors;
/// the automatic launch check stays silent unless there is something to install.
pub fn check(app: &AppHandle, manual: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(&app, manual).await {
            log::warn!("update check failed: {e}");
            if manual {
                info(&app, MessageDialogKind::Error, format!("Couldn't check for updates.\n\n{e}"));
            }
        }
    });
}

async fn run(app: &AppHandle, manual: bool) -> tauri_plugin_updater::Result<()> {
    let Some(update) = app.updater()?.check().await? else {
        if manual {
            info(app, MessageDialogKind::Info, format!("Milkrice {} is up to date.", app.package_info().version));
        }
        return Ok(());
    };
    log::info!("update available: {}", update.version);
    let msg = format!(
        "Milkrice {} is available (you have {}).\n\nInstall it now? Milkrice will restart.",
        update.version, update.current_version
    );
    let a = app.clone();
    let yes = tauri::async_runtime::spawn_blocking(move || {
        a.dialog()
            .message(msg)
            .title("Update available")
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom("Install and restart".into(), "Later".into()))
            .blocking_show()
    })
    .await
    .unwrap_or(false);
    if !yes {
        return Ok(());
    }
    update.download_and_install(|_, _| {}, || {}).await?;
    log::info!("update installed, restarting");
    // Finish any recording in progress before relaunching.
    let a = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || recorder::stop_and_wait(&a, std::time::Duration::from_secs(20))).await;
    app.restart();
}

fn info(app: &AppHandle, kind: MessageDialogKind, msg: String) {
    app.dialog().message(msg).title("Milkrice").kind(kind).show(|_| {});
}
