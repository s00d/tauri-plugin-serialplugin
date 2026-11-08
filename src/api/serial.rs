//! Tauri-managed `SerialPort<R>`: a thin layer over [`serialplugin_core::SerialManager`].
//!
//! Every core method is reachable through `Deref`; only the streaming calls differ,
//! taking a Tauri IPC [`Channel`] (whose id becomes the watch id) instead of a sink.

use crate::error::Error;
use crate::events::{channel_sink, PortListEvent, SerialEvent, WatchOptions, WatchPortsOptions};
use serialplugin_core::SerialManager;
use std::ops::Deref;
use tauri::ipc::Channel;
#[cfg(not(target_os = "android"))]
use tauri::plugin::PluginHandle;
#[cfg(not(target_os = "android"))]
use tauri::AppHandle;
use tauri::Runtime;

/// Access to the serial port APIs (desktop + Android).
pub struct SerialPort<R: Runtime> {
    #[cfg(not(target_os = "android"))]
    #[allow(dead_code)]
    pub(crate) app: AppHandle<R>,
    #[cfg(target_os = "android")]
    _runtime: std::marker::PhantomData<fn() -> R>,
    pub(crate) core: SerialManager,
}

impl<R: Runtime> Clone for SerialPort<R> {
    fn clone(&self) -> Self {
        Self {
            #[cfg(not(target_os = "android"))]
            app: self.app.clone(),
            #[cfg(target_os = "android")]
            _runtime: std::marker::PhantomData,
            core: self.core.clone(),
        }
    }
}

impl<R: Runtime> Deref for SerialPort<R> {
    type Target = SerialManager;

    fn deref(&self) -> &SerialManager {
        &self.core
    }
}

#[cfg(target_os = "android")]
impl<R: Runtime> Default for SerialPort<R> {
    fn default() -> Self {
        Self {
            _runtime: std::marker::PhantomData,
            core: SerialManager::new(),
        }
    }
}

impl<R: Runtime> SerialPort<R> {
    #[cfg(target_os = "android")]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(not(target_os = "android"))]
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            core: SerialManager::new(),
        }
    }

    #[cfg(not(target_os = "android"))]
    #[allow(dead_code)]
    pub fn from_plugin_handle(plugin_handle: PluginHandle<R>) -> Self {
        Self::new(plugin_handle.app().clone())
    }

    /// The Tauri-free core (share it with code that does not know about Tauri).
    pub fn core(&self) -> &SerialManager {
        &self.core
    }

    /// Subscribe to available-port attach/detach events via a Tauri channel.
    pub fn watch_ports(
        &self,
        options: WatchPortsOptions,
        channel: Channel<PortListEvent>,
    ) -> Result<u32, Error> {
        self.core
            .watch_ports_with_id(channel.id(), options, channel_sink(channel))
    }

    /// Stream port data through a Tauri IPC channel. Returns the channel id for `unwatch`.
    pub fn watch(
        &self,
        path: String,
        options: WatchOptions,
        channel: Channel<SerialEvent>,
    ) -> Result<u32, Error> {
        self.core
            .watch_with_id(channel.id(), path, options, channel_sink(channel))
    }
}

impl<R: Runtime> serialplugin_core::at::commands::ExchangeRunner for SerialPort<R> {
    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: crate::events::ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        serialplugin_core::at::commands::ExchangeRunner::run_exchange_unqueued(
            &self.core, path, payload, options,
        )
    }
}
