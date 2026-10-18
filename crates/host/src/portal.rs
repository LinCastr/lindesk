//! xdg-desktop-portal ScreenCast session (the only sanctioned way to capture
//! a Wayland screen). The chosen screen is remembered through a restore
//! token, so the compositor prompt appears once.

use std::os::fd::OwnedFd;

use anyhow::{Context, Result, bail};
use ashpd::desktop::PersistMode;
use ashpd::desktop::screencast::{
    CursorMode, Screencast, SelectSourcesOptions, SourceType, StartCastOptions,
};
use ashpd::enumflags2::BitFlags;

pub struct Portal {
    pub node: u32,
    pub fd: OwnedFd,
    // Dropping the session (inside the runtime) stops the capture.
    rt: tokio::runtime::Runtime,
    session: Option<ashpd::desktop::Session<Screencast>>,
}

fn token_file() -> Option<std::path::PathBuf> {
    Some(directories::ProjectDirs::from("", "", "lindesk")?.config_dir().join("screen-token"))
}

pub fn forget_choice() {
    if let Some(p) = token_file() {
        let _ = std::fs::remove_file(p);
    }
}

/// Opens the session. Shows the compositor's picker only without a saved choice.
pub fn open(cursor: bool) -> Result<Portal> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .context("tokio runtime")?;

    let (session, node, fd) = rt.block_on(async {
        let proxy = Screencast::new().await.context("ScreenCast portal (is xdg-desktop-portal running?)")?;
        let session = proxy.create_session(Default::default()).await.context("create session")?;
        let token = token_file().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.trim().to_string());
        let kinds: BitFlags<SourceType> = SourceType::Monitor | SourceType::Window;
        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_sources(kinds)
                    .set_multiple(false)
                    .set_cursor_mode(if cursor { CursorMode::Embedded } else { CursorMode::Hidden })
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(token.as_deref().filter(|t| !t.is_empty())),
            )
            .await
            .context("select sources")?
            .response()
            .map_err(|e| anyhow::anyhow!("source selection refused: {e}"))?;
        let resp = proxy
            .start(&session, None, StartCastOptions::default())
            .await
            .context("start cast")?
            .response()
            .map_err(|e| anyhow::anyhow!("portal refused: {e}"))?;
        if let (Some(t), Some(p)) = (resp.restore_token(), token_file()) {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, t);
        }
        let Some(stream) = resp.streams().first() else {
            bail!("portal returned no stream");
        };
        let node = stream.pipe_wire_node_id();
        let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await.context("open PipeWire remote")?;
        Ok::<_, anyhow::Error>((session, node, fd))
    })?;
    log::info!("portal: PipeWire node {node}");
    Ok(Portal { node, fd, rt, session: Some(session) })
}

impl Drop for Portal {
    fn drop(&mut self) {
        let _g = self.rt.enter();
        drop(self.session.take());
    }
}
