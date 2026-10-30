//! Lo único que el módem necesita del puerto: escribir bytes y leer lo que haya llegado.
//! Con un trait para poder probar el protocolo AT sin hardware.

use std::io;
use std::time::Duration;

pub trait Enlace: Send {
    fn escribir(&mut self, datos: &[u8]) -> io::Result<()>;
    /// Devuelve lo disponible ahora mismo (puede estar vacío). No bloquea.
    fn leer(&mut self) -> io::Result<Vec<u8>>;
}

impl Enlace for Box<dyn Enlace> {
    fn escribir(&mut self, datos: &[u8]) -> io::Result<()> {
        (**self).escribir(datos)
    }
    fn leer(&mut self) -> io::Result<Vec<u8>> {
        (**self).leer()
    }
}

/// Puerto serie real (COM en Windows, /dev/ttyUSB* en Linux).
pub struct Serie(Box<dyn serialport::SerialPort>);

impl Serie {
    pub fn abrir(nombre: &str) -> Result<Self, String> {
        let p = serialport::new(nombre, 115_200)
            .timeout(Duration::from_millis(20))
            .open()
            .map_err(|e| e.to_string())?;
        Ok(Self(p))
    }
}

impl Enlace for Serie {
    fn escribir(&mut self, datos: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.0.write_all(datos)?;
        self.0.flush()
    }

    fn leer(&mut self) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let n = self.0.bytes_to_read().map_err(io::Error::other)? as usize;
        if n == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; n];
        let leido = match self.0.read(&mut buf) {
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => 0,
            Err(e) => return Err(e),
        };
        buf.truncate(leido);
        Ok(buf)
    }
}
