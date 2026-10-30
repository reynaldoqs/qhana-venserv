//! Módems USB con comandos AT (Huawei, Quectel, SIMCom…) como dispositivos del control.
//!
//! El motor de recetas solo conoce [`nucleo::CanalUssd`]. [`CanalModem`] lo implementa sobre un puerto
//! serie, y [`detector`] encuentra los módems, los registra en el [`nucleo::Registro`] y los da de baja
//! cuando se desenchufan. El contrato está en `docs/protocolo.md` ("Módems USB").
//!
//! `modem/tigo.py` es el prototipo en Python y la referencia de comportamiento.

pub mod at;
pub mod canal;
pub mod detector;
pub mod enlace;
pub mod gsm7;

pub use canal::CanalModem;
pub use detector::{iniciar, Opciones};
