//! Núcleo del control: todo lo que no depende de cómo se conecta un teléfono.
//!
//! - [`canal::CanalUssd`]: la única interfaz que el motor necesita. Hoy la implementa un agente
//!   Android conectado por WebSocket; mañana un módem AT o un teléfono por Wi-Fi.
//! - [`registro::Registro`]: dispositivos conocidos y su canal.
//! - [`eventos::Bus`]: eventos para la interfaz y el historial.
//! - [`receta`] + [`motor`]: flujos USSD declarados en YAML y su ejecución segura.
//! - [`simulacion`]: operadores simulados (Tigo, Entel) para tests y para el agente simulado.

pub mod canal;
pub mod eventos;
pub mod menu;
pub mod motor;
pub mod receta;
pub mod registro;
pub mod simulacion;

pub use canal::{CanalUssd, ErrorCanal};
pub use eventos::Bus;
pub use receta::Receta;
pub use registro::Registro;
