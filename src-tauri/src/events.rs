use sealflash_core::acquire::AcquireEvent;
use sealflash_core::flash::FlashEvent;
use serde::Serialize;

/// Fortschritt von „Forge!“: erst Beschaffen (Phase 3), dann Schreiben.
#[derive(Clone, Serialize)]
#[serde(tag = "source", content = "event", rename_all = "snake_case")]
pub enum ForgeEvent {
    Acquire(AcquireEvent),
    Flash(FlashEvent),
}
