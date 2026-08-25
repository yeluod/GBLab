use gblab_core::CoreInfo;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    app_name: &'static str,
    app_version: &'static str,
    core_version: &'static str,
    database_ready: bool,
}

impl AppInfoDto {
    pub const fn from_core(core: CoreInfo) -> Self {
        Self {
            app_name: "GBLab",
            app_version: env!("CARGO_PKG_VERSION"),
            core_version: core.version,
            database_ready: core.database_ready,
        }
    }
}
